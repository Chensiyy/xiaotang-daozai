use super::*;
use std::{
    io::{BufRead, BufReader, Write},
    net::TcpListener,
    sync::atomic::AtomicUsize,
    thread,
};

const TODAY: &str = "2026-11-22";
const READ: &str = "example.read";
const OTHER: &str = "example.other";

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Result<Self> {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("daozai-read-{}-{unique}", std::process::id()));
        fs::create_dir(&root)?;
        Ok(Self(root))
    }

    fn host(&self) -> LocalPlugins {
        LocalPlugins::open(self.0.join("installed"))
    }

    fn plugin(&self, id: &str, permissions: Value, code: &str) -> Result<PathBuf> {
        let dir = self.0.join(id);
        fs::create_dir(&dir)?;
        fs::write(
            dir.join(COVER_FILE),
            include_bytes!("examples/payday/cover.png"),
        )?;
        fs::write(dir.join("main.js"), code)?;
        let path = dir.join("plugin.json");
        fs::write(
            &path,
            serde_json::to_vec(&json!({
                "id": id, "name": "测试卡片", "author": "示例", "version": "1.0.0",
                "api": 1, "runtime": "js", "entry": "main.js", "owner": "tasks", "cover": COVER_FILE,
                "capabilities": ["card", "timer"], "dataVersion": 1, "permissions": permissions, "settings": []
            }))?,
        )?;
        Ok(path)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn wait(mut until: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(6);
    while !until() {
        assert!(Instant::now() < deadline, "等待后台插件完成超时");
        thread::sleep(Duration::from_millis(2));
    }
}

fn settle(host: &mut LocalPlugins, tasks_on: bool) {
    wait(|| {
        host.tick(tasks_on, false, TODAY);
        host.jobs.is_empty()
    });
}

const SOURCE_CARD: &str =
    "function refresh(i) { return { card: i.source.text ?? i.source.status }; }";

#[test]
fn user_file_grant_reads_only_after_enable_and_is_revoked_on_uninstall() -> Result<()> {
    let fixture = Fixture::new()?;
    let path = fixture.plugin(READ, json!({"source.read":"file"}), SOURCE_CARD)?;
    let file = fixture.0.join("数据.txt");
    fs::write(&file, "\u{feff}文件已读")?;
    let mut host = fixture.host();
    host.install(&path)?;
    host.enable(READ, true)?;
    settle(&mut host, true);
    assert_eq!(host.cards[READ], "unconfigured");

    assert!(host
        .set_source_grant(
            READ,
            Some(source_read::grant_url("http://127.0.0.1/status")?)
        )
        .is_err());
    host.enable(READ, false)?;
    host.set_source_grant(READ, Some(source_read::grant_file(&file)?))?;
    settle(&mut host, true);
    assert!(!host.cards.contains_key(READ));
    drop(host);

    let mut host = fixture.host();
    host.enable(READ, true)?;
    settle(&mut host, true);
    assert_eq!(host.cards[READ], "文件已读");
    fs::rename(&file, fixture.0.join("已移走.txt"))?;
    host.next.remove(READ);
    settle(&mut host, true);
    assert_eq!(
        host.cards.get(READ).map(String::as_str),
        Some("offline"),
        "{:?}",
        host.errors
    );
    host.uninstall(READ, true)?;
    host.install(&path)?;
    assert!(host.source_grant(READ).is_none());
    Ok(())
}

#[test]
fn disabling_an_in_flight_local_request_discards_it_and_preserves_other_cards() -> Result<()> {
    let fixture = Fixture::new()?;
    let mut host = fixture.host();
    host.install(&fixture.plugin(READ, json!({"source.read":"loopback"}), SOURCE_CARD)?)?;
    host.install(&fixture.plugin(
        OTHER,
        json!({}),
        "function refresh() { return { card: '普通卡片' }; }",
    )?)?;
    let listener = TcpListener::bind("127.0.0.1:0")?;
    listener.set_nonblocking(true)?;
    let url = format!("http://{}/status", listener.local_addr()?);
    let requests = Arc::new(AtomicUsize::new(0));
    let release = Arc::new(AtomicBool::new(false));
    let worker_requests = Arc::clone(&requests);
    let worker_release = Arc::clone(&release);
    let server = thread::spawn(move || -> std::io::Result<TcpListener> {
        let mut connection = None;
        wait(|| match listener.accept() {
            Ok((stream, _)) => {
                connection = Some(stream);
                true
            }
            Err(_) => false,
        });
        let mut stream = connection.unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
        let mut request = String::new();
        let mut reader = BufReader::new(&mut stream);
        reader.read_line(&mut request)?;
        assert_eq!(request.trim_end(), "GET /status HTTP/1.1");
        loop {
            request.clear();
            if reader.read_line(&mut request)? == 0 || request == "\r\n" {
                break;
            }
        }
        worker_requests.fetch_add(1, Ordering::Release);
        wait(|| worker_release.load(Ordering::Acquire));
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\nlate")?;
        Ok(listener)
    });

    host.set_source_grant(READ, Some(source_read::grant_url(&url)?))?;
    host.enable(OTHER, true)?;
    settle(&mut host, true);
    assert_eq!(requests.load(Ordering::Acquire), 0);
    assert_eq!(host.cards[OTHER], "普通卡片");

    host.enable(READ, true)?;
    host.next.remove(OTHER);
    wait(|| {
        host.tick(true, false, TODAY);
        requests.load(Ordering::Acquire) == 1 && !host.jobs.contains_key(OTHER)
    });
    assert!(host.jobs.contains_key(READ));
    assert_eq!(host.cards[OTHER], "普通卡片");
    host.enable(READ, false)?;
    release.store(true, Ordering::Release);
    let listener = server.join().unwrap()?;
    settle(&mut host, true);
    assert!(!host.cards.contains_key(READ));
    assert!(
        matches!(listener.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock)
    );
    drop(listener);

    host.enable(READ, true)?;
    settle(&mut host, true);
    assert_eq!(host.cards[READ], "offline");
    assert_eq!(host.cards[OTHER], "普通卡片");
    host.set_source_grant(READ, None)?;
    settle(&mut host, true);
    assert_eq!(host.cards[READ], "unconfigured");
    Ok(())
}

#[test]
fn https_manifest_combination_and_public_sample() -> Result<()> {
    let manifest = include_str!("examples/https-repository/plugin.json");
    let package = Package::from_entry(
        Manifest::decode(manifest)?,
        include_str!("examples/https-repository/main.js").into(),
        CoverPng::decode(include_bytes!("examples/https-repository/cover.png").to_vec())?,
    )?;
    let input = SourceInput::Ready { text: r#"{"stargazers_count":123}"#.into() };
    assert_eq!(
        package.render_input(&Values::new(), &Value::Null, TODAY, input)?.card,
        "Rust · 123 星标"
    );
    let original: Value = serde_json::from_str(manifest)?;
    let mut wrong = original.clone();
    wrong.as_object_mut().unwrap().remove("network");
    assert!(Manifest::decode(&wrong.to_string()).is_err());
    wrong = original.clone();
    wrong["permissions"]["source.read"] = json!("loopback");
    assert!(Manifest::decode(&wrong.to_string()).is_err());
    wrong = original.clone();
    wrong["network"]["hosts"] = json!(["example.com", "EXAMPLE.COM"]);
    assert!(Manifest::decode(&wrong.to_string()).is_err());
    wrong = original;
    wrong["network"]["url"] = json!("https://elsewhere.example.com/status");
    assert!(Manifest::decode(&wrong.to_string()).is_err());
    Ok(())
}

#[test]
fn https_credential_is_external_and_revoke_stops_reads() -> Result<()> {
    let fixture = Fixture::new()?;
    let id = format!("example.https-{}", fixture.0.file_name().unwrap().to_string_lossy());
    let path = fixture.plugin(&id, json!({"source.read":"https"}), SOURCE_CARD)?;
    let mut manifest: Value = serde_json::from_slice(&fs::read(&path)?)?;
    manifest["network"] = json!({
        "hosts": ["example.com"], "url": "https://example.com/data",
        "credential": {"label":"测试密钥", "header":"X-Test-Key", "prefix":"Bearer "}
    });
    fs::write(&path, serde_json::to_vec(&manifest)?)?;
    struct Cleanup<'a>(&'a str);
    impl Drop for Cleanup<'_> {
        fn drop(&mut self) {
            let _ = source_https::delete_secret(self.0);
        }
    }
    let _cleanup = Cleanup(&id);
    let mut host = fixture.host();
    host.install(&path)?;
    let grant = host.records[&id].network().unwrap().grant("https://example.com/data")?;
    let secret = "nonproduction-credential-for-local-test";
    assert!(host.set_source_grant(&id, Some(grant.clone())).is_err());
    host.set_source_grant_with_secret(&id, Some(grant.clone()), Some(secret))?;
    assert_eq!(host.source_grant(&id), Some(&grant));
    assert_eq!(source_https::load_secret(&id)?.as_deref(), Some(secret));
    assert!(!fs::read_to_string(host.path(&id))?.contains(secret));
    drop(host);

    let mut host = fixture.host();
    assert_eq!(host.source_grant(&id), Some(&grant));
    host.set_source_grant(&id, None)?;
    assert!(source_https::load_secret(&id)?.is_none());
    host.enable(&id, true)?;
    settle(&mut host, true);
    assert_eq!(host.cards[&id], "unconfigured");

    host.set_source_grant_with_secret(&id, Some(grant), Some(secret))?;
    host.uninstall(&id, true)?;
    assert!(source_https::load_secret(&id)?.is_none());
    host.install(&path)?;
    assert!(host.source_grant(&id).is_none());
    Ok(())
}
