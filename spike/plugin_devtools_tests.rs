use super::*;

const ID: &str = "example.payday";
const INVOCATION: &str = "JSON.stringify(refresh(__input))";

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Result<Self> {
        let suffix = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)?
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("daozai-devtools-{}-{suffix}", std::process::id()));
        fs::create_dir(&root)?;
        let fixture = Self(root);
        fs::write(
            fixture.0.join("plugin.json"),
            include_str!("examples/payday/plugin.json"),
        )?;
        fs::write(
            fixture.0.join("cover.png"),
            include_bytes!("examples/payday/cover.png"),
        )?;
        fs::write(
            fixture.0.join("main.js"),
            "function refresh() { return {card:'首版'}; }",
        )?;
        Ok(fixture)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn detects_two_saves_after_debounce_preserves_state_and_skips_market() -> Result<()> {
    let fixture = Fixture::new()?;
    let mut plugins = LocalPlugins::open(fixture.0.join("installed"));
    let manifest = fixture.0.join("plugin.json");
    let script = fixture.0.join("main.js");
    plugins.install(&manifest)?;
    plugins.enable(ID, true)?;
    let mut record = plugins.installed(ID)?;
    record.storage = json!({"count": 5});
    record.values.insert("title".into(), "自定义".into());
    plugins.put(ID, record)?;
    plugins.set_watch_files(true);
    let initial = Instant::now();
    assert!(plugins.devtools.watches.contains_key(ID));

    fs::write(&script, "function refresh() { return {card: '保存一次'}; }")?;
    plugins.poll_local_reloads(initial + POLL);
    assert!(plugins.records[ID]
        .package
        .as_ref()
        .unwrap()
        .source
        .contains("首版"));
    let latest = "function refresh(i) { return {card: i.settings.title + i.storage.count}; }";
    fs::write(&script, latest)?;
    plugins.poll_local_reloads(initial + POLL * 2);
    assert!(plugins.records[ID]
        .package
        .as_ref()
        .unwrap()
        .source
        .contains("首版"));
    plugins.poll_local_reloads(initial + POLL * 3);
    let record = plugins.installed(ID)?;
    assert_eq!(record.package.as_ref().unwrap().source, latest);
    assert_eq!(record.storage, json!({"count": 5}));
    assert_eq!(record.values["title"], "自定义");
    assert!(record.enabled && !plugins.next.contains_key(ID));

    let mut market = record;
    market.from_market = true;
    plugins.put(ID, market)?;
    fs::write(
        &script,
        "function refresh() { return {card: '集市不重载'}; }",
    )?;
    plugins.poll_local_reloads(initial + POLL * 4);
    plugins.poll_local_reloads(initial + POLL * 5);
    assert!(!plugins.devtools.watches.contains_key(ID));
    assert_eq!(plugins.records[ID].package.as_ref().unwrap().source, latest);
    Ok(())
}

#[test]
fn data_monitor_accepts_unicode_root_names_and_rejects_reserved_paths() {
    assert!(valid_data_name("2026 法定节假日.json"));
    for name in [
        "../节假日.json",
        "目录\\节假日.json",
        "C:节假日.json",
        "plugin.json",
        "AUX.data.json",
        "COM2.json",
        "LPT1.json",
    ] {
        assert!(!valid_data_name(name), "{name}");
    }
}

#[test]
fn declared_data_and_manifest_changes_are_debounced_together() -> Result<()> {
    let fixture = Fixture::new()?;
    let origin = fixture.0.join("plugin.json");
    let data = fixture.0.join("2026 法定节假日.json");
    let unused = fixture.0.join("notes.json");
    fs::write(&data, "{}")?;
    let files = WatchedPackage::files(
        &fixture.0,
        &origin,
        "main.js",
        &json!({"data": ["2026 法定节假日.json"]}),
    );
    assert!(files.contains(&data) && !files.contains(&unused));
    let mut watch = WatchedPackage {
        stamps: WatchedPackage::read_stamps(&files),
        files,
        changed: None,
    };
    let start = Instant::now();
    fs::write(unused, "{}")?;
    assert!(!watch.poll(start));
    fs::write(data, "{\"days\":[]}")?;
    assert!(!watch.poll(start + POLL));
    assert_eq!(watch.changed, Some(start + POLL));
    fs::write(origin, "{\"changed\":true}")?;
    assert!(!watch.poll(start + POLL * 2));
    assert!(watch.poll(start + POLL * 3));
    assert!(!watch.poll(start + POLL * 4));
    Ok(())
}

#[test]
fn local_reload_releases_a_removed_pinned_capability() -> Result<()> {
    let fixture = Fixture::new()?;
    let path = fixture.0.join("plugin.json");
    let mut manifest: Value = serde_json::from_slice(&fs::read(&path)?)?;
    manifest["capabilities"] = json!(["card", "settings", "island.pinned"]);
    fs::write(&path, manifest.to_string())?;
    let dir = fixture.0.join("installed");
    let mut plugins = LocalPlugins::open(dir.clone());
    plugins.install(&path)?;
    plugins.set_pinned(ID, true)?;
    plugins.set_watch_files(true);
    manifest["capabilities"] = json!(["card", "settings"]);
    fs::write(&path, manifest.to_string())?;
    let start = Instant::now();
    plugins.poll_local_reloads(start + POLL);
    plugins.poll_local_reloads(start + POLL * 2);
    assert!(plugins.pinned.order.is_empty());
    assert!(LocalPlugins::open(dir).pinned.order.is_empty());
    Ok(())
}

#[test]
fn limits_script_logs_and_locates_syntax_and_runtime_errors() -> Result<()> {
    let mut logs = Vec::new();
    let source = "function refresh() { for (let j = 0; j < 25; ++j) console.log('测'.repeat(220), j); return {}; }";
    assert_eq!(
        run_script(
            source.as_bytes(),
            "main.js",
            "{}".into(),
            INVOCATION,
            &mut logs
        )?,
        "{}"
    );
    assert_eq!(logs.len(), 20);
    assert!(logs.iter().all(|line| line.chars().count() == 200));
    let history = ScriptLogs::default();
    for _ in 0..6 {
        history.append(ID, logs.clone());
    }
    assert_eq!(history.snapshot().len(), 100);
    for source in [
        "function refresh() {\n\n  throw new Error('运行错误');\n}",
        "function refresh() {\n\n  let = ;\n}",
    ] {
        let error = run_script(
            source.as_bytes(),
            "main.js",
            "{}".into(),
            INVOCATION,
            &mut logs,
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("main.js:3:"), "{error}");
    }
    Ok(())
}

#[test]
fn plugin_test_runs_one_refresh_with_mock_input_and_reports_capabilities() -> Result<()> {
    let fixture = Fixture::new()?;
    let manifest = fixture.0.join("plugin.json");
    let path = fixture.0.join("mock.json");
    fs::write(
        fixture.0.join("main.js"),
        r#"
function refresh(i) {
    console.log("离线试跑", i.sources.gold.status);
    return {card: [i.settings.title, i.settings.target, i.storage.last, i.now, i.sources.gold.text].join("/")};
}
"#,
    )?;
    fs::write(
        &path,
        json!({
            "settings": {"title": "模拟"},
            "storage": {"last": 3},
            "now": 1795380000000i64,
            "sources": {"gold": {"status": "ready", "text": "718"}}
        })
        .to_string(),
    )?;
    let report = market::test_project(&manifest, Some(&path))?;
    assert!(
        report.contains("模拟/2026-12-25/3/1795380000000/718"),
        "{report}"
    );
    assert!(report.contains("card · 可用"), "{report}");
    assert!(
        report.contains("refresh 用时") && report.contains("离线试跑 ready"),
        "{report}"
    );
    let mut raw: Value = serde_json::from_str(&fs::read_to_string(&manifest)?)?;
    raw["capabilities"] = json!(["card", "unknown.test"]);
    fs::write(&manifest, raw.to_string())?;
    let error = market::test_project(&manifest, Some(&path))
        .unwrap_err()
        .to_string();
    assert!(error.contains("unknown.test · 未支持"), "{error}");
    assert!(
        capability_report(&json!({"runtime":"js","capabilities":["status.page"]}))
            .contains("status.page · 实验")
    );
    Ok(())
}

#[test]
fn plugin_test_supplies_mock_data_to_legacy_single_source_plugins() -> Result<()> {
    let fixture = Fixture::new()?;
    let path = fixture.0.join("plugin.json");
    let mock = fixture.0.join("mock.json");
    let mut manifest: Value = serde_json::from_slice(&fs::read(&path)?)?;
    manifest["permissions"] = json!({"source.read":"file"});
    fs::write(&path, manifest.to_string())?;
    fs::write(
        fixture.0.join("main.js"),
        "function refresh(i) { return {card:[i.source.status, i.source.text, typeof i.now].join('/')}; }",
    )?;
    fs::write(
        &mock,
        json!({"sources": {"default": {"status":"ready","text":"离线"}}}).to_string(),
    )?;
    let report = market::test_project(&path, Some(&mock))?;
    assert!(report.contains("ready/离线/number"), "{report}");
    fs::write(
        &mock,
        json!({"sources": {"default": {"status":"offline"}}}).to_string(),
    )?;
    let report = market::test_project(&path, Some(&mock))?;
    assert!(report.contains("offline//number"), "{report}");
    Ok(())
}
