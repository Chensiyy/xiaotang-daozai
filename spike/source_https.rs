//! HTTPS 数据源的授权校验和受控读取。脚本只接收响应正文。
use super::source_read::{check_cancelled, read_text, SourceGrant};
use serde::{Deserialize, Serialize};
use std::{
    io,
    net::{IpAddr, Ipv4Addr, SocketAddr, ToSocketAddrs},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};
use url::{Host, Url};

const LIMIT: u64 = 256 * 1024;
const TIMEOUT: Duration = Duration::from_secs(10);
const ADDRESS_ERROR: &str = "请输入白名单域名下的 HTTPS 地址，端口使用 443";
const SERVICE: &str = "daozai.plugin-https-source";

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Network {
    pub(super) hosts: Vec<String>,
    #[serde(
        default,
        deserialize_with = "super::present_value",
        skip_serializing_if = "Option::is_none"
    )]
    pub(super) url: Option<String>,
    #[serde(
        default,
        deserialize_with = "super::present_value",
        skip_serializing_if = "Option::is_none"
    )]
    pub(super) credential: Option<Credential>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Credential {
    pub(super) label: String,
    header: String,
    #[serde(
        default,
        deserialize_with = "super::present_value",
        skip_serializing_if = "Option::is_none"
    )]
    prefix: Option<String>,
}

impl Network {
    pub(super) fn valid(&self) -> bool {
        (1..=4).contains(&self.hosts.len())
            && self.hosts.iter().enumerate().all(|(i, host)| {
                valid_host(host)
                    && !self.hosts[..i]
                        .iter()
                        .any(|other| other.eq_ignore_ascii_case(host))
            })
            && self
                .url
                .as_deref()
                .is_none_or(|url| self.address(url).is_ok())
            && self.credential.as_ref().is_none_or(Credential::valid)
    }

    pub(super) fn address(&self, raw: &str) -> io::Result<Url> {
        allowed_url(raw, &self.hosts).ok_or_else(|| io::Error::other(ADDRESS_ERROR))
    }

    pub(super) fn grant(&self, raw: &str) -> io::Result<SourceGrant> {
        Ok(SourceGrant::Https {
            url: self.address(raw.trim())?.into(),
        })
    }

    pub(super) fn read(&self, id: &str, raw: &str, cancelled: &AtomicBool) -> io::Result<String> {
        check_cancelled(cancelled)?;
        let url = self.address(raw)?;
        let secret = match &self.credential {
            Some(config) => {
                let secret =
                    load_secret(id)?.ok_or_else(|| io::Error::other("请重新授权数据源密钥"))?;
                let value = config.header_value(&secret)?;
                Some((config, secret, value))
            }
            None => None,
        };
        let deadline = Instant::now() + TIMEOUT;
        let agent = ureq::AgentBuilder::new()
            .try_proxy_from_env(false)
            .redirects(0)
            .timeout_connect(TIMEOUT)
            .resolver(move |address: &str| resolve_public(address, deadline))
            .build();
        let mut request = agent
            .get(url.as_str())
            .timeout(deadline.saturating_duration_since(Instant::now()))
            .set("Accept", "application/json, text/plain")
            .set("Accept-Encoding", "identity");
        if let Some((config, _, value)) = &secret {
            request = request.set_sensitive(&config.header, value);
        }
        check_cancelled(cancelled)?;
        let response = request
            .call()
            .map_err(|_| io::Error::other("HTTPS 数据源无法读取"))?;
        let text = response_text(response, cancelled)?;
        if secret
            .as_ref()
            .is_some_and(|(_, secret, _)| text.contains(secret))
        {
            return Err(io::Error::other("HTTPS 数据源返回了凭据"));
        }
        Ok(text)
    }
}

impl Credential {
    fn valid(&self) -> bool {
        !self.label.trim().is_empty()
            && self.label.chars().count() <= 80
            && !self.label.chars().any(char::is_control)
            && (1..=64).contains(&self.header.len())
            && self
                .header
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_\x60|~".contains(&byte))
            && ![
                "Host",
                "Connection",
                "Content-Length",
                "Transfer-Encoding",
                "TE",
                "Upgrade",
                "Trailer",
                "Expect",
                "Accept-Encoding",
                "Proxy-Authorization",
                "Proxy-Connection",
            ]
            .iter()
            .any(|name| self.header.eq_ignore_ascii_case(name))
            && self.prefix.as_ref().is_none_or(|prefix| {
                prefix.len() <= 80 && prefix.bytes().all(|b| matches!(b, b' '..=b'~'))
            })
    }

    pub(super) fn header_value(&self, secret: &str) -> io::Result<String> {
        if !(1..=2048).contains(&secret.len())
            || secret.trim() != secret
            || !secret.bytes().all(|b| matches!(b, b' '..=b'~'))
        {
            return Err(io::Error::other("密钥须为 1–2048 个可打印 ASCII 字符"));
        }
        Ok(format!(
            "{}{secret}",
            self.prefix.as_deref().unwrap_or_default()
        ))
    }
}

fn valid_host(host: &str) -> bool {
    host.len() <= 253
        && host.contains('.')
        && !host.to_ascii_lowercase().ends_with(".localhost")
        && host.split('.').all(|label| {
            (1..=63).contains(&label.len())
                && label.as_bytes()[0].is_ascii_alphanumeric()
                && label.as_bytes()[label.len() - 1].is_ascii_alphanumeric()
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
        && matches!(Host::parse(host), Ok(Host::Domain(domain)) if domain.eq_ignore_ascii_case(host))
}

fn allowed_url(raw: &str, hosts: &[String]) -> Option<Url> {
    if raw.len() > 2048
        || raw.chars().any(|c| c.is_control() || c == '\\')
        || !raw.get(..8)?.eq_ignore_ascii_case("https://")
        || raw[8..]
            .split(['/', '?', '#'])
            .next()
            .is_none_or(|authority| authority.is_empty() || authority.contains('@'))
    {
        return None;
    }
    let url = Url::parse(raw).ok()?;
    let Host::Domain(host) = url.host()? else {
        return None;
    };
    (url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url.port_or_known_default() == Some(443)
        && url.fragment().is_none()
        && hosts
            .iter()
            .any(|allowed| host.eq_ignore_ascii_case(allowed)))
    .then_some(url)
}

fn public_v4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !matches!(
        (a, b, c),
        (0 | 10 | 127 | 224..=255, _, _)
            | (100, 64..=127, _)
            | (169, 254, _)
            | (172, 16..=31, _)
            | (192, 0, 0 | 2)
            | (192, 168, _)
            | (198, 18 | 19, _)
            | (198, 51, 100)
            | (203, 0, 113)
    )
}

fn public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => public_v4(ip),
        IpAddr::V6(ip) => ip.to_ipv4_mapped().map_or_else(
            || {
                let [a, b, ..] = ip.segments();
                // 仅接受全球单播地址，排除 Teredo、6to4 及文档地址。
                (0x2000..=0x3fff).contains(&a)
                    && !matches!((a, b), (0x2001, 0 | 0xdb8) | (0x2002 | 0x3fff, _))
            },
            public_v4,
        ),
    }
}

fn public_addresses(addresses: impl Iterator<Item = SocketAddr>) -> io::Result<Vec<SocketAddr>> {
    let addresses: Vec<_> = addresses.filter(|addr| public_ip(addr.ip())).collect();
    if addresses.is_empty() {
        return Err(io::Error::other("HTTPS 域名未解析到公网地址"));
    }
    Ok(addresses)
}

fn resolve_public(address: &str, deadline: Instant) -> io::Result<Vec<SocketAddr>> {
    // 系统 DNS 是同步调用。最多保留一个查询线程，请求超时后立即结束等待。
    static RESOLVING: AtomicBool = AtomicBool::new(false);
    let (host, port) = address
        .rsplit_once(':')
        .ok_or_else(|| io::Error::other(ADDRESS_ERROR))?;
    if port != "443" || !valid_host(host) {
        return Err(io::Error::other(ADDRESS_ERROR));
    }
    let target = format!("{host}.:443");
    if RESOLVING.swap(true, Ordering::AcqRel) {
        return Err(io::Error::other("系统 DNS 正在查询"));
    }
    let (send, receive) = mpsc::sync_channel(1);
    if let Err(error) = std::thread::Builder::new()
        .name("daozai-plugin-dns".into())
        .spawn(move || {
            let addresses = target.to_socket_addrs().and_then(public_addresses);
            RESOLVING.store(false, Ordering::Release);
            let _ = send.send(addresses);
        })
    {
        RESOLVING.store(false, Ordering::Release);
        return Err(error);
    }
    receive
        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "HTTPS 域名解析超时"))?
}

fn response_text(response: ureq::Response, cancelled: &AtomicBool) -> io::Result<String> {
    if !(200..300).contains(&response.status()) {
        return Err(io::Error::other("HTTPS 数据源没有直接返回数据"));
    }
    read_text(response.into_reader(), cancelled, LIMIT)
}

fn keyring_entry(id: &str) -> io::Result<keyring::Entry> {
    keyring::Entry::new(SERVICE, id).map_err(|_| io::Error::other("无法访问 Windows 凭据管理器"))
}

pub(super) fn load_secret(id: &str) -> io::Result<Option<String>> {
    match keyring_entry(id)?.get_password() {
        Ok(value) => Ok(Some(value)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(_) => Err(io::Error::other("无法读取数据源密钥")),
    }
}

pub(super) fn save_secret(id: &str, secret: &str) -> io::Result<()> {
    keyring_entry(id)?
        .set_password(secret)
        .map_err(|_| io::Error::other("无法保存数据源密钥"))
}

pub(super) fn delete_secret(id: &str) -> io::Result<()> {
    match keyring_entry(id)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(_) => Err(io::Error::other("无法删除数据源密钥")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domains_and_addresses_must_match_the_https_grant() {
        let hosts = vec!["example.com".to_owned()];
        for host in [
            "*",
            "*.example.com",
            "127.0.0.1",
            "[::1]",
            "127.1",
            "localhost",
            "api.localhost",
            "example.com:443",
            "example.com.",
        ] {
            assert!(!valid_host(host), "{host}");
        }
        assert!(valid_host("API.Example.com"));
        for raw in [
            "http://example.com",
            "https://example.net",
            "https://sub.example.com",
            "https://127.0.0.1",
            "https://user:secret@example.com",
            "https://@example.com",
            "https://example.com:444",
            "https://example.com/#part",
            "https:///example.com",
            "https://example.com.evil.net",
        ] {
            assert!(allowed_url(raw, &hosts).is_none(), "{raw}");
        }
        assert_eq!(
            allowed_url("https://EXAMPLE.COM:443/path?arg=1", &hosts)
                .unwrap()
                .as_str(),
            "https://example.com/path?arg=1"
        );
    }

    #[test]
    fn resolver_uses_only_public_addresses() {
        let blocked = [
            "0.0.0.0",
            "10.0.0.1",
            "100.64.0.1",
            "127.0.0.1",
            "169.254.1.2",
            "172.31.0.1",
            "192.168.1.1",
            "224.0.0.1",
            "::",
            "::1",
            "fe80::1",
            "fc00::1",
            "ff02::1",
            "::ffff:127.0.0.1",
            "::ffff:192.168.0.1",
        ];
        let make = |s: &str| SocketAddr::new(s.parse().unwrap(), 443);
        assert!(public_addresses(blocked.map(make).into_iter()).is_err());
        let public = ["8.8.8.8", "2001:4860:4860::8888"].map(make);
        let all = blocked.map(make).into_iter().chain(public);
        assert_eq!(public_addresses(all).unwrap(), public);
    }

    #[test]
    fn redirects_oversize_and_invalid_text_are_rejected() {
        let cancelled = AtomicBool::new(false);
        let redirect = ureq::Response::new(302, "Found", "redirect").unwrap();
        assert!(response_text(redirect, &cancelled).is_err());
        let body = "a".repeat(LIMIT as usize);
        let response = ureq::Response::new(200, "OK", &body).unwrap();
        assert_eq!(response_text(response, &cancelled).unwrap(), body);
        let body = "a".repeat(LIMIT as usize + 1);
        let response = ureq::Response::new(200, "OK", &body).unwrap();
        assert!(response_text(response, &cancelled).is_err());
        assert!(read_text(&[0xff][..], &cancelled, LIMIT).is_err());
    }

    #[test]
    fn credential_headers_are_validated_and_redacted() {
        let mut config = Credential {
            label: "密钥".into(),
            header: "X-Service-Key".into(),
            prefix: Some("Bearer ".into()),
        };
        assert!(config.valid());
        assert!(config.header_value("abc\r\nHost: other.example").is_err());
        let header = config.header_value("example-private-value").unwrap();
        let request = ureq::get("https://example.com").set_sensitive(&config.header, &header);
        assert_eq!(request.header(&config.header), Some(header.as_str()));
        let debug = format!("{request:?}");
        assert!(debug.contains("[REDACTED]"));
        assert!(!debug.contains("example-private-value"));
        config.header = "hOsT".into();
        assert!(!config.valid());
    }
}
