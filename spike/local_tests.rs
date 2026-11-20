use super::*;
fn settle(host: &mut LocalPlugins, today: &str) {
    let end = Instant::now() + Duration::from_secs(5);
    loop {
        host.tick(true, today);
        if host.jobs.is_empty() {
            break;
        }
        assert!(Instant::now() < end);
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn two_packages_survive_disable_restart_and_both_uninstall_choices() -> Result<()> {
    let root = std::env::temp_dir().join(format!(
        "daozai-local-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    ));
    let dir = root.join("installed");
    let mut host = LocalPlugins::open(dir.clone());
    let sample = Path::new(env!("CARGO_MANIFEST_DIR")).join("spike/examples/payday/plugin.json");
    let trip = root.join("source");
    fs::create_dir_all(&trip)?;
    let second = trip.join("plugin.json");
    let mut m: Value = serde_json::from_slice(&fs::read(&sample)?)?;
    m["id"] = json!("example.trip");
    m["name"] = json!("旅行");
    m["settings"][0]["default"] = json!("旅行");
    fs::write(&second, serde_json::to_vec(&m)?)?;
    fs::write(
        trip.join("main.js"),
        "function refresh(i){return {card:i.settings.title,storage:(i.storage||0)+1}}",
    )?;
    let (a, b) = ("example.payday", "example.trip");
    host.install(&sample)?;
    host.install(&second)?;
    assert!(host.install(&sample).is_err());
    settle(&mut host, "2026-12-01");
    assert!(host.cards.is_empty());
    host.enable(a, true)?;
    host.enable(b, true)?;
    let mut v = host.records[a].values.clone();
    v.insert("target".into(), "2026-12-03".into());
    host.save(a, v)?;
    settle(&mut host, "2026-12-01");
    assert_eq!(host.cards[a], "距离发薪还有 2 天");
    assert_eq!(host.cards[b], "旅行");
    host.enable(b, false)?;
    host.tick(false, "");
    assert!(host.cards.is_empty());
    drop(host);
    let mut host = LocalPlugins::open(dir.clone());
    host.tick(false, "");
    assert!(host.cards.is_empty());
    settle(&mut host, "2026-12-02");
    assert_eq!(host.cards[a], "距离发薪还有 1 天");
    assert!(!host.cards.contains_key(b));
    assert_eq!(host.records[b].storage, 1);
    host.enable(b, true)?;
    settle(&mut host, "2026-12-02");
    assert_eq!(host.records[b].storage, 2);
    host.uninstall(a, true)?;
    assert!(!host.cards.contains_key(a));
    assert!(host.cards.contains_key(b));
    host.install(&sample)?;
    assert_eq!(host.records[a].values["target"], "2026-12-03");
    assert_eq!(host.records[a].storage["lastDays"], 1);
    host.uninstall(a, false)?;
    host.install(&sample)?;
    assert_eq!(host.records[a].values["target"], "2026-12-25");
    assert!(host.records[a].storage.is_null());
    assert_eq!(host.records[b].storage, 2);
    m["entry"] = json!("../outside.js");
    fs::write(&second, serde_json::to_vec(&m)?)?;
    assert!(Package::read(&second).is_err());
    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn deadline_work_never_blocks_frames_or_starts_twice() -> Result<()> {
    let dir = std::env::temp_dir().join(format!("daozai-frame-test-{}", std::process::id()));
    let mut host = LocalPlugins::open(dir.clone());
    let sample = Path::new(env!("CARGO_MANIFEST_DIR")).join("spike/examples/payday/plugin.json");
    host.install(&sample)?;
    let id = "example.payday";
    host.records
        .get_mut(id)
        .unwrap()
        .package
        .as_mut()
        .unwrap()
        .source = "function refresh(){while(true){}}".into();
    host.enable(id, true)?;
    let start = Instant::now();
    host.tick(true, "2026-11-19");
    let worker = host.jobs[id].result.thread().id();
    assert_ne!(worker, std::thread::current().id());
    for _ in 0..10 {
        host.tick(true, "2026-11-19");
        assert_eq!(host.jobs[id].result.thread().id(), worker);
    }
    assert!(
        start.elapsed() < Duration::from_millis(25),
        "十次界面更新不应等待 30 毫秒的插件"
    );
    settle(&mut host, "2026-11-19");
    assert!(host.errors.contains_key(id));
    fs::remove_dir_all(dir)?;
    Ok(())
}

#[test]
fn delayed_result_discarded_after_disable_uninstall_and_restart_validation() -> Result<()> {
    let dir = std::env::temp_dir().join(format!("daozai-late-test-{}", std::process::id()));
    let mut host = LocalPlugins::open(dir.clone());
    let sample = Path::new(env!("CARGO_MANIFEST_DIR")).join("spike/examples/payday/plugin.json");
    let id = "example.payday";
    host.install(&sample)?;
    host.enable(id, true)?;
    host.tick(true, "2026-11-19");
    host.enable(id, false)?;
    settle(&mut host, "2026-11-19");
    assert!(host.cards.is_empty());
    assert!(host.records[id].storage.is_null());
    assert!(LocalPlugins::open(dir.clone()).records[id]
        .storage
        .is_null());
    host.enable(id, true)?;
    host.tick(true, "2026-11-19");
    host.uninstall(id, true)?;
    host.install(&sample)?;
    settle(&mut host, "2026-11-19");
    assert!(host.cards.is_empty());
    assert!(host.records[id].storage.is_null());
    assert!(LocalPlugins::open(dir.clone()).records[id].storage.is_null());
    let mut broken = host.records[id].clone();
    broken.enabled = true;
    broken.package.as_mut().unwrap().manifest.entry = "../other.js".into();
    host.put(id, broken)?;
    drop(host);
    let mut host = LocalPlugins::open(dir.clone());
    host.tick(true, "2026-11-19");
    assert!(host.invalid.contains(id));
    assert!(host.jobs.is_empty());
    assert!(host.cards.is_empty());
    host.install(&sample)?;
    assert!(host.invalid.is_empty());
    fs::remove_dir_all(dir)?;
    Ok(())
}

#[test]
fn design_card_example_runs_with_both_installed_manifests() -> Result<()> {
    let design = include_str!("../docs/plugins/sdk-v1-design.md");
    let source = design
        .split_once("```js")
        .unwrap()
        .1
        .split_once("```")
        .unwrap()
        .0;
    for sample in ["payday", "trip"] {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("spike/examples");
        let mut pkg = Package::read(&root.join(sample).join("plugin.json"))?;
        let values: Values = pkg
            .manifest
            .settings
            .iter()
            .map(|f| (f.key.clone(), f.default.clone()))
            .collect();
        let first = pkg.render(&values, &Value::Null, "2026-11-19")?;
        pkg.source = source.to_owned();
        let doc_example = pkg.render(&values, &first.storage, "2026-11-19")?;
        assert_eq!(doc_example.storage, first.storage);
        assert!(doc_example.card.contains(&values["title"]));
    }
    Ok(())
}
