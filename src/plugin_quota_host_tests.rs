use super::*;

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        static SERIAL: AtomicU64 = AtomicU64::new(0);
        let serial = SERIAL.fetch_add(1, Ordering::Relaxed);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "daozai-quota-host-{}-{now}-{serial}",
            std::process::id()
        ));
        fs::create_dir(&root).unwrap();
        Self(root)
    }

    fn package(&self, id: &str, permissions: Value, script: &str) -> PathBuf {
        let root = self.0.join(id);
        fs::create_dir(&root).unwrap();
        fs::write(root.join("main.js"), script).unwrap();
        image::DynamicImage::new_rgb8(1, 1)
            .save(root.join(COVER_FILE))
            .unwrap();
        let path = root.join("plugin.json");
        let manifest = json!({
            "id": id, "name": "模拟额度", "author": "测试", "version": "1.0.0",
            "api": 1, "dataVersion": 1, "owner": "quota", "runtime": "js",
            "entry": "main.js", "cover": COVER_FILE,
            "capabilities": ["quota.source", "timer", "storage", "settings"],
            "permissions": permissions,
            "settings": [{"key": "label", "label": "额度名称", "type": "text", "default": "积分"}]
        });
        fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        path
    }

    fn host(&self) -> PluginQuotaHost {
        PluginQuotaHost::open(self.0.join("local-plugins"))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn settle(host: &mut PluginQuotaHost, on: bool) {
    let deadline = Instant::now() + Duration::from_secs(6);
    loop {
        host.tick(on);
        if host.jobs.is_empty() {
            break;
        }
        assert!(Instant::now() < deadline, "等待额度来源超时");
        thread::sleep(Duration::from_millis(2));
    }
}

fn reading(host: &PluginQuotaHost, id: &str) -> Option<f64> {
    let (_, _, quota) = host.quota_sources().find(|(source, _, _)| *source == id)?;
    quota.windows.first()?.remaining_pct
}

fn status(host: &PluginQuotaHost, id: &str) -> PluginQuotaStatus {
    host.records()
        .find(|record| record.id == id)
        .unwrap()
        .status
}

#[test]
fn quota_host_loads_only_valid_quota_records_and_persists_private_storage() {
    let fixture = Fixture::new();
    let id = "test.quota-healthy";
    let manifest = fixture.package(
        id,
        json!({}),
        r#"function refresh({settings,storage}) {
            const remaining_pct=(storage?.run || 0) + 1;
            return {quota:{windows:[{label:settings.label,remaining_pct}]},storage:{run:remaining_pct}};
        }"#,
    );
    let invalid = fixture.package("test.quota-permission", json!({"net.fetch":true}), "");
    let mut host = fixture.host();
    assert!(host.install(&invalid).is_err());
    assert_eq!(host.install(&manifest).unwrap(), id);
    assert!(host.install(&manifest).is_err());
    assert_eq!(status(&host, id), PluginQuotaStatus::Disabled);
    assert!(host.grant_url(id, "http://127.0.0.1/status").is_err());

    host.enable(id, true).unwrap();
    settle(&mut host, true);
    assert_eq!(reading(&host, id), Some(1.0));
    assert_eq!(status(&host, id), PluginQuotaStatus::Ready);
    let path = host.path(id);
    let persisted: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(persisted["storage"]["run"], 1);
    assert!(persisted.get("quota").is_none());
    drop(host);

    let mut host = fixture.host();
    assert_eq!(reading(&host, id), None);
    settle(&mut host, true);
    assert_eq!(reading(&host, id), Some(2.0));
    host.pause();
    assert_eq!(reading(&host, id), None);
    settle(&mut host, false);
    assert_eq!(reading(&host, id), None);
    host.forget(id).unwrap();
    assert!(host.records().next().is_none());
    assert!(!path.exists());
}

#[test]
fn quota_host_reinstalls_only_its_own_retained_local_record() {
    let fixture = Fixture::new();
    let id = "test.quota-retained";
    let manifest = fixture.package(id, json!({}), "function refresh() { return {quota:null}; }");
    let mut host = fixture.host();
    fs::create_dir(&host.dir).unwrap();
    let path = host.path(id);
    let old = json!({
        "origin": manifest.canonicalize().unwrap(),
        "package": null,
        "enabled": false,
        "values": {"label":"保留的名称", "removed":"不再声明"},
        "storage": {"run":5},
        "from_market": false,
        "material_granted": false
    });
    for (key, value) in [
        ("origin", json!(fixture.0.join("different.json"))),
        ("package", json!({"manifest":{"owner":"tasks"}})),
        ("enabled", json!(true)),
        ("from_market", json!(true)),
        ("official_market", json!(true)),
        ("material_granted", json!(true)),
        ("presentation", json!({"title":"集市记录"})),
        (
            "source_grant",
            json!({"Loopback":"http://127.0.0.1/status"}),
        ),
    ] {
        let mut other = old.clone();
        other[key] = value;
        let bytes = serde_json::to_vec(&other).unwrap();
        fs::write(&path, &bytes).unwrap();
        assert!(host.install(&manifest).is_err(), "拒绝记录字段：{key}");
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }

    fs::write(&path, serde_json::to_vec(&old).unwrap()).unwrap();
    assert_eq!(host.install(&manifest).unwrap(), id);
    assert_eq!(status(&host, id), PluginQuotaStatus::Disabled);
    assert!(host.records[id].source_grant.is_none());
    assert_eq!(host.records[id].values["label"], "保留的名称");
    assert_eq!(host.records[id].values.len(), 1);
    assert_eq!(host.records[id].storage, json!({"run":5}));
    host.forget(id).unwrap();

    let mut without_storage: Value = serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
    without_storage["capabilities"]
        .as_array_mut()
        .unwrap()
        .retain(|capability| capability.as_str() != Some("storage"));
    fs::write(&manifest, serde_json::to_vec(&without_storage).unwrap()).unwrap();
    let mut old = old;
    old["values"]["label"] = json!(" ");
    fs::write(&path, serde_json::to_vec(&old).unwrap()).unwrap();
    host.install(&manifest).unwrap();
    assert_eq!(host.records[id].values["label"], "积分");
    assert!(host.records[id].storage.is_null());
}

#[cfg(windows)]
#[test]
fn quota_host_file_disconnect_withdraws_only_that_source() {
    let fixture = Fixture::new();
    let id = "test.quota-file";
    let other = "test.quota-other";
    let manifest = fixture.package(
        id,
        json!({"source.read":"file"}),
        r#"function refresh({source}) {
            return {quota:{windows:[{
                label:"模拟额度",
                remaining_pct:source.status==="ready" ? Number(source.text) : 90
            }]}};
        }"#,
    );
    let other_manifest = fixture.package(
        other,
        json!({}),
        r#"function refresh() { return {quota:{windows:[{label:"独立额度",remaining_pct:42}]}}; }"#,
    );
    let data = fixture.0.join("readings.txt");
    fs::write(&data, "68").unwrap();
    let mut host = fixture.host();
    host.install(&manifest).unwrap();
    host.install(&other_manifest).unwrap();
    assert!(host.grant_url(id, "http://127.0.0.1/status").is_err());
    host.enable(id, true).unwrap();
    host.enable(other, true).unwrap();
    settle(&mut host, true);
    assert_eq!(reading(&host, id), None);
    assert_eq!(status(&host, id), PluginQuotaStatus::Unconfigured);
    assert_eq!(reading(&host, other), Some(42.0));

    host.grant_file(id, &data).unwrap();
    settle(&mut host, true);
    assert_eq!(reading(&host, id), Some(68.0));
    fs::remove_file(&data).unwrap();
    host.refresh();
    settle(&mut host, true);
    assert_eq!(status(&host, id), PluginQuotaStatus::Offline);
    assert_eq!(reading(&host, id), None);
    assert_eq!(reading(&host, other), Some(42.0));
    fs::write(&data, "57").unwrap();
    host.refresh();
    settle(&mut host, true);
    assert_eq!(reading(&host, id), Some(57.0));
    host.enable(id, false).unwrap();
    assert_eq!(reading(&host, id), None);
    host.refresh();
    settle(&mut host, true);
    assert_eq!(status(&host, id), PluginQuotaStatus::Disabled);
    assert_eq!(reading(&host, other), Some(42.0));
}

#[cfg(windows)]
#[test]
fn quota_host_revoking_an_in_flight_loopback_discards_its_output_and_storage() {
    use std::{
        io::{BufRead, BufReader, Write},
        net::TcpListener,
        sync::mpsc,
    };

    let fixture = Fixture::new();
    let id = "test.quota-loopback";
    let manifest = fixture.package(
        id,
        json!({"source.read":"loopback"}),
        r#"function refresh({source}) {
            if(source.status!=="ready") return {quota:null};
            return {quota:{windows:[{label:"本机额度",remaining_pct:Number(source.text)}]},storage:{received:true}};
        }"#,
    );
    let server = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = server.local_addr().unwrap();
    let (request, requested) = mpsc::channel();
    let (release, released) = mpsc::channel();
    let responder = thread::spawn(move || {
        let (mut stream, _) = server.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut lines = BufReader::new(&stream).lines();
        assert_eq!(lines.next().unwrap().unwrap(), "GET /quota HTTP/1.1");
        for line in lines {
            if line.unwrap().is_empty() {
                break;
            }
        }
        request.send(()).unwrap();
        released.recv_timeout(Duration::from_secs(2)).unwrap();
        let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n99");
    });

    let mut host = fixture.host();
    host.install(&manifest).unwrap();
    host.grant_url(id, &format!("http://{addr}/quota")).unwrap();
    host.enable(id, true).unwrap();
    host.tick(true);
    requested.recv_timeout(Duration::from_secs(2)).unwrap();
    host.revoke(id).unwrap();
    assert_eq!(status(&host, id), PluginQuotaStatus::Unconfigured);
    release.send(()).unwrap();
    responder.join().unwrap();
    settle(&mut host, true);
    assert_eq!(reading(&host, id), None);
    assert!(host.records[id].storage.is_null());
    let saved: Value = serde_json::from_slice(&fs::read(host.path(id)).unwrap()).unwrap();
    assert!(saved.get("source_grant").is_none());
    assert!(saved["storage"].is_null());
}

#[test]
fn quota_host_isolates_invalid_quota_payloads() {
    let fixture = Fixture::new();
    let id = "test.quota-invalid";
    let manifest = fixture.package(
        id,
        json!({}),
        r#"function refresh() { return {quota:{windows:[{label:"错误额度",remaining_pct:101}]}}; }"#,
    );
    let mut host = fixture.host();
    host.install(&manifest).unwrap();
    host.enable(id, true).unwrap();
    settle(&mut host, true);
    assert_eq!(reading(&host, id), None);
    assert_eq!(status(&host, id), PluginQuotaStatus::Error);
}
