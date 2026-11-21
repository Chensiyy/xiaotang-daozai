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

#[cfg(windows)]
mod test_market {
    use super::*;
    use crate::sdk_spike::local::market_state::MarketStatus;

    const ID: &str = "example.payday";

    struct Fixture {
        root: PathBuf,
        manifest_path: PathBuf,
        index: PathBuf,
    }

    impl Fixture {
        fn new() -> Result<Self> {
            let root = std::env::temp_dir().join(format!(
                "daozai-market-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)?
                    .as_nanos()
            ));
            let source = root.join("source");
            let market_dir = root.join("market");
            fs::create_dir_all(&source)?;
            fs::create_dir_all(&market_dir)?;
            let sample = Path::new(env!("CARGO_MANIFEST_DIR")).join("spike/examples/payday");
            let manifest_path = source.join("plugin.json");
            fs::copy(sample.join("plugin.json"), &manifest_path)?;
            fs::copy(sample.join("main.js"), source.join("main.js"))?;
            fs::copy(sample.join(COVER_FILE), source.join(COVER_FILE))?;
            Ok(Self {
                root,
                manifest_path,
                index: market_dir.join("catalog.json"),
            })
        }

        fn publish(&self, version: &str, incompatible: bool) -> Result<()> {
            let mut manifest: Value = serde_json::from_slice(&fs::read(&self.manifest_path)?)?;
            manifest["version"] = json!(version);
            if incompatible {
                manifest["settings"][0]["type"] = json!("date");
                manifest["settings"][0]["default"] = json!("2026-12-01");
            }
            fs::write(&self.manifest_path, serde_json::to_vec(&manifest)?)?;
            let zip = self.root.join(format!("submit-{version}.zip"));
            market::pack(&self.manifest_path, &zip)?;
            market::submit(&zip, &self.index)
        }

        fn install(&self, host: &mut LocalPlugins) -> Result<()> {
            self.publish("1.0.0", false)?;
            host.set_market_catalog(&self.index)?;
            host.market_action(false, ID)?;
            settle_market(host);
            assert_eq!(status(host), MarketStatus::Installed);
            assert!(!host.records[ID].enabled);
            Ok(())
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn settle_market(host: &mut LocalPlugins) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while host.market_pending() {
            host.poll_market();
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn status(host: &LocalPlugins) -> MarketStatus {
        let catalog = host.market_catalog(false).unwrap();
        host.market_status(&catalog, &catalog.entries[0])
    }

    #[test]
    fn market_favorites_survive_a_restart_and_delisting() -> Result<()> {
        let fixture = Fixture::new()?;
        fixture.publish("1.0.0", false)?;
        let entry = market::Catalog::open(&fixture.index)?.entries.remove(0);
        let storage = fixture.root.join("installed");
        let mut host = LocalPlugins::open(storage.clone());
        assert!(host.favorites().is_empty());
        host.set_favorite(&entry, true)?;
        host.set_favorite(&entry, true)?;
        assert!(host.is_favorite(ID));
        assert_eq!(host.favorites().len(), 1);

        // 删除在线目录后，收藏仍可显示名称并取消。
        fs::remove_file(&fixture.index)?;
        let mut host = LocalPlugins::open(storage.clone());
        assert_eq!(host.favorites().get(ID), Some(&entry.name));
        host.remove_favorite(ID)?;
        assert!(LocalPlugins::open(storage).favorites().is_empty());
        Ok(())
    }

    #[test]
    fn market_install_update_restart_and_reinstall_keep_data() -> Result<()> {
        let fixture = Fixture::new()?;
        let storage = fixture.root.join("installed");
        let mut host = LocalPlugins::open(storage.clone());
        fixture.install(&mut host)?;
        let previous_index = fs::read(&fixture.index)?;
        host.enable(ID, true)?;
        let mut values = host.records[ID].values.clone();
        values.insert("title".into(), "我的发薪日".into());
        host.save(ID, values.clone())?;
        settle(&mut host, "2026-12-01");
        let saved = host.records[ID].storage.clone();
        assert!(!saved.is_null());
        host.tick(false, "2026-12-02");
        assert!(host.records[ID].enabled && !host.has_card());

        // 无封面的旧快照从磁盘恢复，在线更新后随包保存封面。
        let mut old: Value = serde_json::from_slice(&fs::read(host.path(ID))?)?;
        let snapshot = old["package"].as_object_mut().unwrap();
        snapshot.remove("cover");
        snapshot
            .get_mut("manifest")
            .unwrap()
            .as_object_mut()
            .unwrap()
            .remove("cover");
        fs::write(host.path(ID), serde_json::to_vec(&old)?)?;
        let mut host = LocalPlugins::open(storage.clone());
        assert!(host.records[ID]
            .package
            .as_ref()
            .unwrap()
            .cover_png()
            .is_none());
        assert!(host.records[ID].enabled);
        assert_eq!(host.records[ID].values, values);

        fixture.publish("1.1.0", false)?;
        host.refresh_market_catalog()?;
        assert_eq!(status(&host), MarketStatus::Update);
        host.tick(true, "2026-12-02");
        host.market_action(false, ID)?;
        settle_market(&mut host);
        assert!(!host.jobs[ID].valid, "上个版本的迟到结果应作废");
        assert_eq!(host.records[ID].storage, saved);
        assert_eq!(status(&host), MarketStatus::Installed);
        assert!(host.records[ID].enabled);
        assert_eq!(host.records[ID].values, values);
        settle(&mut host, "2026-12-01");

        let mut host = LocalPlugins::open(storage);
        assert_eq!(status(&host), MarketStatus::Installed);
        assert_eq!(host.records[ID].values, values);
        assert_eq!(host.records[ID].storage, saved);
        assert!(host.records[ID].enabled);
        assert_eq!(
            host.records[ID]
                .package
                .as_ref()
                .unwrap()
                .cover_png()
                .as_deref(),
            Some(include_bytes!("examples/payday/cover.png").as_slice())
        );
        host.uninstall(ID, true)?;
        host.market_action(false, ID)?;
        settle_market(&mut host);
        assert_eq!(host.records[ID].values, values);
        assert_eq!(host.records[ID].storage, saved);
        assert!(!host.records[ID].enabled);

        fs::write(&fixture.index, previous_index)?;
        host.refresh_market_catalog()?;
        assert_eq!(status(&host), MarketStatus::Unavailable);
        assert!(host.market_action(false, ID).is_err());
        Ok(())
    }

    #[test]
    fn market_bad_or_cancelled_update_never_replaces_installed_record() -> Result<()> {
        let fixture = Fixture::new()?;
        let mut host = LocalPlugins::open(fixture.root.join("installed"));
        fixture.install(&mut host)?;
        host.enable(ID, true)?;
        let previous = fs::read(host.path(ID))?;

        fixture.publish("1.1.0", false)?;
        let zip = fixture
            .index
            .with_file_name(format!("plugin-{ID}-1.1.0.zip"));
        let mut bytes = fs::read(&zip)?;
        bytes[0] ^= 1;
        fs::write(&zip, bytes)?;
        host.refresh_market_catalog()?;
        host.market_action(false, ID)?;
        settle_market(&mut host);
        assert!(host.notice.contains("更新失败"));
        assert_eq!(fs::read(host.path(ID))?, previous);

        fixture.publish("1.2.0", true)?;
        host.refresh_market_catalog()?;
        host.market_action(false, ID)?;
        settle_market(&mut host);
        assert!(host.notice.contains("字段类型"));
        assert_eq!(fs::read(host.path(ID))?, previous);

        host.market_action(false, ID)?;
        host.enable(ID, false)?;
        let disabled = fs::read(host.path(ID))?;
        settle_market(&mut host);
        assert!(host.notice.contains("插件状态已变化"));
        assert_eq!(fs::read(host.path(ID))?, disabled);
        assert!(!host.records[ID].enabled);
        Ok(())
    }

    #[test]
    fn official_market_updates_from_its_mirror_and_keeps_verified_state_on_failure() -> Result<()> {
        use base64::{engine::general_purpose::STANDARD, Engine};
        use std::{
            io::{BufRead, BufReader, Cursor, Write},
            net::TcpListener,
        };

        let fixture = Fixture::new()?;
        fixture.publish("1.0.0", false)?;
        let mut first: Value = serde_json::from_slice(&fs::read(&fixture.index)?)?;
        let avatar_name = "avatar-author.png";
        first["plugins"][0]["presentation"] = json!({
            "official": true,
            "sample": true,
            "maintainer": "renly1994",
            "author_avatar": avatar_name
        });
        let first = serde_json::to_vec(&first)?;
        let mut avatar = Cursor::new(Vec::new());
        image::RgbaImage::new(1, 1).write_to(&mut avatar, image::ImageFormat::Png)?;
        let avatar = avatar.into_inner();
        let first_name = format!("plugin-{ID}-1.0.0.zip");
        let first_zip = fs::read(fixture.index.with_file_name(&first_name))?;
        fixture.publish("1.1.0", false)?;
        let second = fs::read(&fixture.index)?;
        let second_name = format!("plugin-{ID}-1.1.0.zip");
        let second_zip = fs::read(fixture.index.with_file_name(&second_name))?;
        let mut corrupt = second_zip.clone();
        corrupt[0] ^= 1;

        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        let index_url = url::Url::parse(&format!(
            "http://{}/api/v5/repos/owner/plugins/contents/preview/catalog.json?ref=master",
            listener.local_addr()?
        ))?;
        let replies = [
            ("catalog.json", first, false),
            (avatar_name, avatar.clone(), false),
            (first_name.as_str(), first_zip.clone(), false),
            (first_name.as_str(), first_zip, false),
            ("catalog.json", second.clone(), false),
            (second_name.as_str(), corrupt, false),
            (second_name.as_str(), second_zip, false),
            ("catalog.json", Vec::new(), true),
        ]
        .map(|(name, body, redirect)| {
            let path = format!("preview/{name}");
            let expected =
                format!("GET /api/v5/repos/owner/plugins/contents/{path}?ref=master HTTP/1.1");
            let wrapper = json!({
                "type": "file", "encoding": "base64", "path": path,
                "size": body.len(), "content": STANDARD.encode(&body)
            });
            (expected, serde_json::to_vec(&wrapper).unwrap(), redirect)
        });
        let server = std::thread::spawn(move || {
            for (expected, body, redirect) in replies {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut request = BufReader::new(&stream);
                let mut line = String::new();
                request.read_line(&mut line).unwrap();
                assert_eq!(line.trim_end(), expected);
                loop {
                    line.clear();
                    if request.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
                        break;
                    }
                }
                if redirect {
                    write!(
                        stream,
                        "HTTP/1.1 302 Found\r\nLocation: https://example.invalid/outside\r\n"
                    )
                    .unwrap();
                } else {
                    write!(stream, "HTTP/1.1 200 OK\r\n").unwrap();
                }
                write!(
                    stream,
                    "Content-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                )
                .unwrap();
                stream.write_all(&body).unwrap();
            }
        });

        let storage = fixture.root.join("installed");
        let mut host = LocalPlugins::open(storage.clone());
        host.set_market_catalog(&fixture.index)?;
        host.set_official_test_index(index_url.clone());
        host.ensure_official_catalog();
        settle_market(&mut host);
        let remote = host.market_catalog(true).unwrap();
        assert_eq!(remote.entries[0].version, "1.0.0");
        assert_eq!(
            host.market_catalog(false).unwrap().entries[0].version,
            "1.1.0"
        );
        let local = host.market_catalog(false).unwrap();
        assert!(matches!(
            host.official_avatar(&local, &remote.entries[0]),
            Some(Err(()))
        ));
        assert!(host.official_avatar(&remote, &remote.entries[0]).is_none());
        settle_market(&mut host);
        let image = host
            .official_avatar(&remote, &remote.entries[0])
            .unwrap()
            .unwrap();
        assert_eq!(image.bytes(), avatar.as_slice());
        assert!(host.official_cover(&remote, &remote.entries[0]).is_none());
        settle_market(&mut host);
        assert!(matches!(
            host.official_cover(&remote, &remote.entries[0]),
            Some(Ok(_))
        ));
        host.market_action(true, ID)?;
        settle_market(&mut host);
        assert!(host.records[ID].official_market);
        assert!(host.records[ID].presentation.as_ref().unwrap().sample);
        assert!(!host.records[ID].enabled);
        assert_eq!(status(&host), MarketStatus::OtherSource);
        host.enable(ID, true)?;
        let mut values = host.records[ID].values.clone();
        values.insert("title".into(), "我的发薪日".into());
        host.save(ID, values.clone())?;
        let installed = fs::read(host.path(ID))?;

        host.refresh_official_catalog();
        settle_market(&mut host);
        let remote = host.market_catalog(true).unwrap();
        assert_eq!(
            host.market_status(&remote, &remote.entries[0]),
            MarketStatus::Update
        );
        host.market_action(true, ID)?;
        settle_market(&mut host);
        assert!(host.notice.contains("SHA-256"));
        assert_eq!(fs::read(host.path(ID))?, installed);
        host.market_action(true, ID)?;
        settle_market(&mut host);
        assert_eq!(
            host.market_status(&remote, &remote.entries[0]),
            MarketStatus::Installed
        );
        assert_eq!(host.records[ID].values, values);
        assert!(host.records[ID].enabled);

        let mut host = LocalPlugins::open(storage);
        host.set_official_test_index(index_url);
        host.ensure_official_catalog();
        settle_market(&mut host);
        let remote = host.market_catalog(true).unwrap();
        assert!(host
            .official_catalog_error()
            .unwrap()
            .contains("非预期跳转"));
        assert_eq!(remote.entries[0].version, "1.1.0");
        assert_eq!(fs::read(&remote.path)?, second);
        server.join().unwrap();
        Ok(())
    }
}

#[test]
fn local_cover_requires_a_bounded_png_inside_the_package() -> Result<()> {
    use image::{codecs::png::PngEncoder, ExtendedColorType, ImageEncoder};

    let root = std::env::temp_dir().join(format!("daozai-cover-test-{}", std::process::id()));
    let package = root.join("package");
    fs::create_dir_all(&package)?;
    let path = package.join("plugin.json");
    let cover = package.join(COVER_FILE);
    let mut manifest: Value = serde_json::from_str(include_str!("examples/payday/plugin.json"))?;
    fs::write(
        package.join("main.js"),
        include_str!("examples/payday/main.js"),
    )?;
    fs::write(&path, serde_json::to_vec(&manifest)?)?;
    assert!(Package::read(&path).is_err());
    fs::write(&cover, b"broken")?;
    assert!(Package::read(&path).is_err());
    fs::write(&cover, vec![0; cover::COVER_LIMIT as usize + 1])?;
    assert!(Package::read(&path)
        .err()
        .unwrap()
        .to_string()
        .contains("256 KiB"));

    let mut too_wide = Vec::new();
    PngEncoder::new(&mut too_wide).write_image(&[0; 1921], 1921, 1, ExtendedColorType::L8)?;
    fs::write(&cover, too_wide)?;
    assert!(Package::read(&path).is_err());

    let png = include_bytes!("examples/payday/cover.png");
    let outside = root.join("outside.png");
    fs::write(&outside, png)?;
    fs::remove_file(&cover)?;
    fs::hard_link(&outside, &cover)?;
    assert!(Package::read(&path).is_err());
    fs::remove_file(&cover)?;
    fs::write(&cover, png)?;

    manifest["cover"] = json!("../outside.png");
    fs::write(&path, serde_json::to_vec(&manifest)?)?;
    assert!(Package::read(&path).is_err());
    manifest.as_object_mut().unwrap().remove("cover");
    fs::write(&path, serde_json::to_vec(&manifest)?)?;
    assert!(Package::read(&path).is_err());
    manifest["cover"] = json!(COVER_FILE);
    fs::write(&path, serde_json::to_vec(&manifest)?)?;
    assert_eq!(
        Package::read(&path)?.cover_png().as_deref(),
        Some(png.as_slice())
    );
    fs::remove_dir_all(root)?;
    Ok(())
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
    fs::copy(sample.with_file_name(COVER_FILE), trip.join(COVER_FILE))?;
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
    assert!(LocalPlugins::open(dir.clone()).records[id]
        .storage
        .is_null());
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

#[cfg(feature = "sdk-material")]
mod material_tests {
    use super::*;

    const SHADER: &str = "fn material(i: MaterialIn) -> vec4<f32> { return vec4<f32>(0.0); }";

    fn root() -> PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("daozai-material-{}-{nonce}", std::process::id()))
    }

    fn manifest(id: &str) -> Value {
        json!({
            "id": id, "name": "测试材质", "author": "测试", "version": "1.0.0", "api": 1,
            "runtime": "material", "entry": "shaders/material.wgsl", "cover": COVER_FILE,
            "owner": "appearance",
            "capabilities": ["appearance.material", "settings"], "dataVersion": 1,
            "permissions": {"screen.backdrop": true},
            "material": {"controls": true, "spring": {"stiffness": 1.0, "damping": 0.5}},
            "settings": [
                {"key": "strength", "label": "强度", "type": "number", "min": 0, "max": 120, "step": 1, "default": "48"},
                {"key": "blur", "label": "模糊", "type": "number", "min": 0, "max": 10, "step": 0.5, "default": "1.5"}
            ]
        })
    }

    fn package(dir: &Path, id: &str) -> Result<PathBuf> {
        let shader_dir = dir.join("shaders");
        fs::create_dir_all(&shader_dir)?;
        fs::write(shader_dir.join("material.wgsl"), SHADER)?;
        fs::write(
            dir.join(COVER_FILE),
            include_bytes!("examples/liquid-glass/cover.png"),
        )?;
        fs::create_dir_all(dir.join("docs"))?;
        fs::write(dir.join("docs/README.md"), "材质说明")?;
        let path = dir.join("plugin.json");
        fs::write(&path, serde_json::to_vec(&manifest(id))?)?;
        Ok(path)
    }

    #[test]
    fn material_manifest_rejects_invalid_settings_and_capabilities() -> Result<()> {
        let valid = manifest("example.material");
        serde_json::from_value::<Manifest>(valid.clone())?.validate()?;

        for key in ["permissions", "material", "settings"] {
            let mut missing = valid.clone();
            missing.as_object_mut().unwrap().remove(key);
            assert!(
                serde_json::from_value::<Manifest>(missing)
                    .ok()
                    .is_none_or(|manifest| manifest.validate().is_err()),
                "{key}"
            );
        }
        for key in ["min", "max", "step"] {
            let mut missing = valid.clone();
            missing["settings"][0].as_object_mut().unwrap().remove(key);
            assert!(
                serde_json::from_value::<Manifest>(missing)?
                    .validate()
                    .is_err(),
                "{key}"
            );
        }
        for (pointer, replacement) in [
            ("/permissions/screen.backdrop", json!(false)),
            ("/material/spring/stiffness", json!(3.1)),
            ("/material/spring/damping", json!(0.0)),
            ("/owner", json!("tasks")),
            ("/capabilities", json!(["card", "settings"])),
            ("/capabilities", json!(["appearance.material", "network"])),
            (
                "/capabilities",
                json!(["appearance.material", "appearance.material"]),
            ),
            ("/capabilities", json!(["appearance.material"])),
            ("/settings/0/type", json!("text")),
            ("/settings/0/min", json!(150)),
            ("/settings/0/step", json!(0)),
            ("/settings/0/step", json!(150)),
            ("/settings/0/default", json!("128")),
            ("/settings/0/default", json!("NaN")),
            ("/entry", json!("../material.wgsl")),
            ("/entry", json!("material.js")),
        ] {
            let mut invalid = valid.clone();
            *invalid.pointer_mut(pointer).unwrap() = replacement;
            assert!(
                serde_json::from_value::<Manifest>(invalid)?
                    .validate()
                    .is_err(),
                "{pointer}"
            );
        }
        let mut too_many = valid;
        too_many["settings"] = Value::Array((0..33).map(|index| {
            json!({"key":format!("param{index}"),"label":"参数","type":"number","min":0,"max":1,"step":0.1,"default":"0"})
        }).collect());
        assert!(serde_json::from_value::<Manifest>(too_many.clone())?
            .validate()
            .is_err());
        too_many["settings"].as_array_mut().unwrap().pop();
        serde_json::from_value::<Manifest>(too_many)?.validate()?;
        Ok(())
    }

    #[test]
    fn material_manifest_accepts_optional_units_and_display_precision() -> Result<()> {
        let mut valid = manifest("example.material");
        serde_json::from_value::<Manifest>(valid.clone())?.validate()?;
        for (unit, precision) in [("px", 0), ("毫秒单位", 3)] {
            valid["settings"][0]["unit"] = json!(unit);
            valid["settings"][0]["precision"] = json!(precision);
            serde_json::from_value::<Manifest>(valid.clone())?.validate()?;
        }
        for (key, value) in [
            ("unit", json!("毫秒单位符")),
            ("unit", json!(3)),
            ("unit", Value::Null),
            ("precision", json!(-1)),
            ("precision", json!(4)),
            ("precision", json!(1.5)),
            ("precision", Value::Null),
        ] {
            let mut invalid = valid.clone();
            invalid["settings"][0][key] = value;
            assert!(
                serde_json::from_value::<Manifest>(invalid)
                    .ok()
                    .is_none_or(|manifest| manifest.validate().is_err()),
                "{key}"
            );
        }
        Ok(())
    }

    #[test]
    fn future_material_api_prompts_to_update_daozai_at_installation() -> Result<()> {
        let root = root();
        let path = package(&root.join("package"), "example.material")?;
        let mut host = LocalPlugins::open(root.join("installed"));
        let mut future = manifest("example.material");
        future["unexpectedFutureField"] = json!(true);
        fs::write(&path, serde_json::to_vec(&future)?)?;
        assert!(!host
            .install(&path)
            .unwrap_err()
            .to_string()
            .contains("需要更新岛仔"));

        future["api"] = json!(2);
        fs::write(&path, serde_json::to_vec(&future)?)?;
        assert!(host
            .install(&path)
            .unwrap_err()
            .to_string()
            .contains("需要更新岛仔"));
        assert!(!host.records.contains_key("example.material"));
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn material_manifest_validates_optional_shadow_and_foreground() -> Result<()> {
        let mut value = manifest("example.material");
        let old = serde_json::from_value::<Manifest>(value.clone())?
            .material
            .unwrap();
        assert_eq!(old.shadow, None);
        assert!(matches!(old.foreground, MaterialForeground::Skin));
        for (shadow, foreground) in [(0.0, "skin"), (1.0, "glass"), (1.5, "glass")] {
            value["material"]["shadow"] = json!(shadow);
            value["material"]["foreground"] = json!(foreground);
            serde_json::from_value::<Manifest>(value.clone())?.validate()?;
        }
        for (key, invalid) in [
            ("shadow", json!(-0.001)),
            ("shadow", json!(1.501)),
            ("shadow", json!("1.0")),
            ("shadow", Value::Null),
            ("foreground", json!("opaque")),
            ("foreground", json!(true)),
            ("foreground", Value::Null),
        ] {
            let mut invalid_manifest = value.clone();
            invalid_manifest["material"][key] = invalid;
            assert!(
                serde_json::from_value::<Manifest>(invalid_manifest)
                    .ok()
                    .is_none_or(|manifest| manifest.validate().is_err()),
                "{key}"
            );
        }
        Ok(())
    }

    #[test]
    fn material_package_allows_only_the_manifest_shader_and_documentation() -> Result<()> {
        let root = root();
        let path = package(&root, "example.material")?;
        assert_eq!(Package::read(&path)?.source, SHADER);
        for name in [
            "main.js",
            "extra.wgsl",
            "helper.exe",
            "docs/asset.bin",
            "docs/plugin.json",
        ] {
            let extra = root.join(name);
            fs::write(&extra, "多余文件")?;
            assert!(Package::read(&path).is_err(), "{name}");
            fs::remove_file(extra)?;
        }
        let shader = root.join("shaders/material.wgsl");
        fs::write(&shader, "a".repeat(MATERIAL_SOURCE_LIMIT as usize))?;
        assert_eq!(
            Package::read(&path)?.source.len() as u64,
            MATERIAL_SOURCE_LIMIT
        );
        fs::write(&shader, "a".repeat(MATERIAL_SOURCE_LIMIT as usize + 1))?;
        assert!(Package::read(&path).is_err());
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    #[cfg(windows)]
    fn liquid_glass_package_installs_and_keeps_parameters_when_disabled() -> Result<()> {
        let root = root();
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("spike/examples/liquid-glass/plugin.json");
        let manifest = Package::read(&path)?.manifest;
        let id = &manifest.id;
        let first_parameter = &manifest.settings[0].key;
        let mut host = LocalPlugins::open(root.clone());
        host.install(&path)?;
        assert!(host.enable(id, true).is_err());
        assert!(host.active_material().is_none());
        assert!(!LocalPlugins::startup_material(&root));

        host.activate_material(Some(id), true)?;
        assert!(LocalPlugins::startup_material(&root));
        let active = host.active_material().unwrap();
        assert!(active.controls);
        assert!(active.glass_foreground);
        assert_eq!(active.shadow, Some(1.0));
        assert_eq!((active.stiffness, active.damping), (1.0, 0.5));
        let mut expected = [48.0, 8.0, 1.5, 87.0, 1.5, 0.9, 1.0];
        assert_eq!(active.params[..expected.len()], expected);
        expected[0] = 49.0;
        host.set_material_parameter(id, first_parameter, expected[0])?;
        assert_eq!(
            host.active_material().unwrap().params[..expected.len()],
            expected
        );
        host.enable(id, false)?;
        assert!(host.active_material().is_none());
        assert!(!LocalPlugins::startup_material(&root));

        let mut host = LocalPlugins::open(root.clone());
        assert!(host.active_material().is_none());
        host.enable(id, true)?;
        assert_eq!(
            host.active_material().unwrap().params[..expected.len()],
            expected
        );
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    #[cfg(windows)]
    fn an_authorized_material_is_selected_exclusively_and_reloaded_with_its_parameters(
    ) -> Result<()> {
        let root = root();
        let storage = root.join("installed");
        let (a, b) = ("example.first", "example.second");
        let path_a = package(&root.join("a"), a)?;
        let path_b = package(&root.join("b"), b)?;
        let mut host = LocalPlugins::open(storage.clone());
        host.install(&path_a)?;
        host.install(&path_b)?;
        assert!(!LocalPlugins::startup_material(&storage));
        assert!(host.enable(a, true).is_err());
        fs::write(
            storage.join("active-material.json"),
            serde_json::to_vec(&a)?,
        )?;
        assert!(!LocalPlugins::startup_material(&storage));
        assert!(LocalPlugins::open(storage.clone())
            .active_material()
            .is_none());

        host.activate_material(Some(a), true)?;
        assert!(LocalPlugins::startup_material(&storage));
        assert_eq!(host.active_material().unwrap().params[..2], [48.0, 1.5]);
        assert!(host.activate_material(Some(b), false).is_err());
        assert_eq!(host.selected_material_id(), Some(a));
        host.activate_material(Some(b), true)?;
        assert_eq!(host.active_material().unwrap().id, b);
        assert!(host.material_permission_granted(a));
        host.enable(a, false)?;
        assert_eq!(host.selected_material_id(), Some(b));
        assert!(host.set_material_parameter(b, "strength", 121.0).is_err());
        host.set_material_parameter(b, "blur", 2.5)?;
        assert_eq!(host.active_material().unwrap().params[..2], [48.0, 2.5]);
        host.tick(true, "2026-11-21");
        assert!(host.jobs.is_empty());

        let mut host = LocalPlugins::open(storage.clone());
        assert_eq!(host.active_material().unwrap().params[..2], [48.0, 2.5]);
        host.activate_material(None, false)?;
        assert!(!LocalPlugins::startup_material(&storage));
        host.activate_material(Some(b), false)?;
        host.uninstall(b, true)?;
        assert!(host.active_material().is_none());
        host.install(&path_b)?;
        assert!(host.activate_material(Some(b), false).is_err());
        host.activate_material(Some(b), true)?;
        assert_eq!(host.active_material().unwrap().params[1], 2.5);
        fs::remove_dir_all(root)?;
        Ok(())
    }
}
