use super::*;
use std::{
    io::{BufRead, BufReader, Write},
    net::TcpListener,
    sync::mpsc,
    thread,
};

const PRINTER: &str = "fixture.bambu";
const OTHER: &str = "fixture.other-status";
const TASKS: &str = "fixture.tasks";
const TODAY: &str = "2026-11-22";
const SOURCE_STATUS: &str = "
    function refresh(i) {
        if (i.source.status !== 'ready') return {status:null};
        return JSON.parse(i.source.text);
    }";

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Result<Self> {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "daozai-structured-status-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir(&root)?;
        Ok(Self(root))
    }

    fn host(&self) -> LocalPlugins {
        LocalPlugins::open(self.0.join("installed"))
    }

    fn plugin(
        &self,
        id: &str,
        owner: &str,
        capabilities: &[&str],
        permissions: Value,
        code: &str,
    ) -> Result<PathBuf> {
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
                "id": id, "name": id, "author": "验收", "version": "1.0.0",
                "api": 1, "runtime": "js", "entry": "main.js", "owner": owner, "cover": COVER_FILE,
                "capabilities": capabilities, "dataVersion": 1, "permissions": permissions, "settings": []
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
        assert!(Instant::now() < deadline, "等待结构化状态刷新超时");
        thread::sleep(Duration::from_millis(2));
    }
}

fn settle(host: &mut LocalPlugins, tasks_on: bool) {
    wait(|| {
        host.tick_features(tasks_on, false, false, TODAY);
        host.jobs.is_empty()
    });
}

fn status(host: &LocalPlugins, id: &str) -> (Option<f64>, StatusHealth) {
    let (_, snapshot, health) = host.status_page(id).expect("已启用的状态页应仍可打开");
    (snapshot.and_then(|snapshot| snapshot.progress_pct), health)
}

fn snapshot(progress: u8) -> String {
    json!({"status":{
        "state":"running", "label":"打印中", "progress_pct":progress,
        "metrics":[{"label":"喷嘴温度","value":"220 °C"},{"label":"层数","value":"74 / 180"}]
    }})
    .to_string()
}

#[test]
fn structured_status_file_disconnect_and_revoke_are_isolated() -> Result<()> {
    let fixture = Fixture::new()?;
    let mut host = fixture.host();
    for id in [PRINTER, OTHER] {
        host.install(&fixture.plugin(
            id,
            "status",
            &["status.page", "timer"],
            json!({"source.read":"file"}),
            SOURCE_STATUS,
        )?)?;
        host.enable(id, true)?;
        assert_eq!(status(&host, id), (None, StatusHealth::Unconfigured));
    }
    host.install(&fixture.plugin(
        TASKS,
        "tasks",
        &["card"],
        json!({}),
        "function refresh() {return {card:'原有待办卡片'};}",
    )?)?;
    host.enable(TASKS, true)?;
    let printer = fixture.0.join("模拟打印机.json");
    let other = fixture.0.join("其他来源.json");
    fs::write(&printer, snapshot(42))?;
    fs::write(&other, snapshot(25))?;
    host.set_source_grant(PRINTER, Some(source_read::grant_file(&printer)?))?;
    host.set_source_grant(OTHER, Some(source_read::grant_file(&other)?))?;
    settle(&mut host, true);
    assert_eq!(status(&host, PRINTER), (Some(42.0), StatusHealth::Ready));
    assert_eq!(status(&host, OTHER), (Some(25.0), StatusHealth::Ready));
    assert_eq!(host.cards[TASKS], "原有待办卡片");

    fs::remove_file(&printer)?;
    fs::write(&other, snapshot(63))?;
    host.next.remove(PRINTER);
    host.next.remove(OTHER);
    settle(&mut host, true);
    assert_eq!(status(&host, PRINTER), (None, StatusHealth::Offline));
    assert_eq!(status(&host, OTHER), (Some(63.0), StatusHealth::Ready));
    assert_eq!(host.cards[TASKS], "原有待办卡片");

    fs::write(&printer, snapshot(58))?;
    host.next.remove(PRINTER);
    settle(&mut host, true);
    assert_eq!(status(&host, PRINTER), (Some(58.0), StatusHealth::Ready));
    host.set_source_grant(PRINTER, None)?;
    assert_eq!(status(&host, PRINTER), (None, StatusHealth::Unconfigured));
    settle(&mut host, true);
    assert!(!host.next.contains_key(PRINTER));
    assert_eq!(status(&host, OTHER), (Some(63.0), StatusHealth::Ready));
    assert_eq!(host.status_pages().count(), 2);
    Ok(())
}

#[test]
fn stopping_only_the_status_page_cancels_an_in_flight_read_and_persists() -> Result<()> {
    let fixture = Fixture::new()?;
    let mut host = fixture.host();
    host.install(&fixture.plugin(
        PRINTER,
        "status",
        &["status.page", "timer", "storage"],
        json!({"source.read":"loopback"}),
        SOURCE_STATUS,
    )?)?;
    host.install(&fixture.plugin(
        OTHER,
        "status",
        &["status.page"],
        json!({}),
        "function refresh() {return {status:{state:'idle',label:'另一个插件',progress_pct:17}};}",
    )?)?;
    host.enable(PRINTER, true)?;
    host.enable(OTHER, true)?;
    let listener = TcpListener::bind("127.0.0.1:0")?;
    listener.set_nonblocking(true)?;
    let url = format!("http://{}/status", listener.local_addr()?);
    host.set_source_grant(PRINTER, Some(source_read::grant_url(&url)?))?;
    let (received, pending) = mpsc::channel();
    let (resume, hold) = mpsc::channel();
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
        let mut reader = BufReader::new(&mut stream);
        let mut line = String::new();
        reader.read_line(&mut line)?;
        assert_eq!(line.trim_end(), "GET /status HTTP/1.1");
        loop {
            line.clear();
            if reader.read_line(&mut line)? == 0 || line == "\r\n" {
                break;
            }
        }
        received.send(()).map_err(std::io::Error::other)?;
        hold.recv_timeout(Duration::from_secs(5))
            .map_err(std::io::Error::other)?;
        let payload = json!({
            "status":{"state":"succeeded","label":"迟到的更新","progress_pct":100},
            "storage":{"should_not_persist":true}
        })
        .to_string();
        let _ = write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
            payload.len()
        );
        Ok(listener)
    });

    host.tick_features(false, false, false, TODAY);
    pending.recv_timeout(Duration::from_secs(3))?;
    assert!(host.jobs.contains_key(PRINTER));
    host.set_status_page_enabled(PRINTER, false)?;
    assert!(host.status_page(PRINTER).is_none());
    assert!(host.records[PRINTER].enabled);
    assert_eq!(host.status_pages().collect::<Vec<_>>(), [(OTHER, OTHER)]);
    resume.send(())?;
    let listener = server.join().map_err(|_| fail("测试回环服务退出"))??;
    settle(&mut host, false);
    assert!(host.records[PRINTER].storage.is_null());
    assert!(!host.status.contains_key(PRINTER));
    assert_eq!(status(&host, OTHER), (Some(17.0), StatusHealth::Ready));
    assert!(
        matches!(listener.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock)
    );

    drop(host);
    let mut host = fixture.host();
    settle(&mut host, false);
    assert!(host.records[PRINTER].enabled);
    assert!(host.status_page(PRINTER).is_none());
    assert_eq!(status(&host, OTHER), (Some(17.0), StatusHealth::Ready));
    assert!(
        matches!(listener.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock)
    );
    drop(listener);

    host.set_status_page_enabled(PRINTER, true)?;
    settle(&mut host, false);
    assert_eq!(status(&host, PRINTER), (None, StatusHealth::Offline));
    host.enable(PRINTER, false)?;
    assert!(host.status_page(PRINTER).is_none());
    assert_eq!(status(&host, OTHER), (Some(17.0), StatusHealth::Ready));
    Ok(())
}

#[test]
fn structured_status_discards_old_data_and_rejects_other_owners() -> Result<()> {
    let fixture = Fixture::new()?;
    let mut host = fixture.host();
    host.install(&fixture.plugin(
        PRINTER,
        "status",
        &["status.page", "timer"],
        json!({"source.read":"file"}),
        SOURCE_STATUS,
    )?)?;
    let file = fixture.0.join("模拟打印机.json");
    fs::write(
        &file,
        r#"{"status":{"state":"idle","label":"旧数据","expires_at":1}}"#,
    )?;
    host.enable(PRINTER, true)?;
    host.set_source_grant(PRINTER, Some(source_read::grant_file(&file)?))?;
    settle(&mut host, false);
    assert_eq!(status(&host, PRINTER), (None, StatusHealth::Stale));

    fs::write(&file, snapshot(35))?;
    host.next.remove(PRINTER);
    settle(&mut host, false);
    assert_eq!(status(&host, PRINTER), (Some(35.0), StatusHealth::Ready));
    fs::write(&file, snapshot(101))?;
    host.next.remove(PRINTER);
    settle(&mut host, false);
    assert_eq!(status(&host, PRINTER), (None, StatusHealth::Invalid));
    fs::write(&file, r#"{"status":null}"#)?;
    host.next.remove(PRINTER);
    settle(&mut host, false);
    assert_eq!(status(&host, PRINTER), (None, StatusHealth::Waiting));

    let foreign = fixture.plugin(
        "fixture.foreign-status",
        "tasks",
        &["card"],
        json!({}),
        "function refresh(){return {card:'原有卡片',status:{state:'idle',label:'伪造状态'}};}",
    )?;
    assert!(Package::read(&foreign)?
        .render(&Values::new(), &Value::Null, TODAY)
        .is_err());
    let unauthorized = fixture.plugin(
        "fixture.no-status-capability",
        "status",
        &["card"],
        json!({}),
        "function refresh(){return {card:'状态'};}",
    )?;
    assert!(Package::read(&unauthorized).is_err());
    Ok(())
}
