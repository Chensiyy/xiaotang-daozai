use super::*;

#[test]
fn market_package_preserves_declared_data() -> Result<()> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("spike/examples/holidays/plugin.json");
    let zip = std::env::temp_dir().join(format!("daozai-package-data-{}.zip", std::process::id()));
    let result = (|| -> Result<()> {
        pack(&manifest, &zip)?;
        let package = decode_zip(&fs::read(&zip)?)?;
        assert!(package.data.contains_key("holidays-2026.json"));
        assert_eq!(package.render(&Values::new(), &Value::Null, "2026-06-19")?.marker, "端午节放假");
        Ok(())
    })();
    let _ = fs::remove_file(zip);
    result
}
use super::super::{LocalPlugins, WIRELESS_PLUGIN_ID};
use serde_json::json;

#[test]
fn gitee_contents_accepts_only_the_requested_bounded_file() -> Result<()> {
    let path = "stable/catalog.json";
    let valid = json!({
        "type": "file",
        "encoding": "base64",
        "size": 3,
        "path": path,
        "content": "YW\r\nJj"
    });
    assert_eq!(
        decode_gitee_file(&serde_json::to_vec(&valid)?, path, 3)?,
        b"abc"
    );
    for (key, value) in [
        ("type", json!("symlink")),
        ("encoding", json!("utf8")),
        ("path", json!("preview/catalog.json")),
        ("size", json!(4)),
        ("size", json!(2)),
        ("content", json!("******")),
        ("content", json!("****")),
    ] {
        let mut invalid = valid.clone();
        invalid[key] = value;
        assert!(decode_gitee_file(&serde_json::to_vec(&invalid)?, path, 3).is_err());
    }
    Ok(())
}

#[test]
fn raw_fallback_requires_rate_limiting_and_the_same_gitee_file() -> Result<()> {
    assert!(gitee_rate_limited(429, std::io::empty()));
    assert!(gitee_rate_limited(
        403,
        &b"Forbidden (Rate Limit Exceeded)"[..]
    ));
    assert!(!gitee_rate_limited(403, &b"Forbidden"[..]));
    assert!(!gitee_rate_limited(404, &b"Rate Limit Exceeded"[..]));
    assert!(!gitee_rate_limited(403, &vec![b'x'; 4096][..]));

    let base = "https://gitee.com/api/v5/repos/xiaotangtoken/xiaotang-daozai-plugins/contents/";
    let source = Url::parse(&format!("{base}preview/catalog.json?ref=master"))?;
    let raw = gitee_raw_url(&source).unwrap();
    let raw_path = "/xiaotangtoken/xiaotang-daozai-plugins/raw/master/preview/catalog.json";
    assert_eq!(raw.as_str(), format!("https://gitee.com{raw_path}"));
    assert!(gitee_raw_url(&Url::parse(&format!(
        "{base}stable/avatar-author.png?ref=master"
    ))?)
    .is_some());
    assert!(gitee_raw_url(&Url::parse(&format!(
        "{base}stable/plugin-example.payday-1.0.0.zip?ref=master"
    ))?)
    .is_some());
    for invalid in [
        format!("{base}preview/catalog.json?ref=other"),
        format!("{base}preview/catalog.json?ref=master&ref=other"),
        format!("{base}unapproved/catalog.json?ref=master"),
        format!("{base}preview/nested/catalog.json?ref=master"),
        format!("{base}preview/index.json?ref=master"),
        format!("{base}preview/catalog.json?ref=master#fragment"),
        source
            .as_str()
            .replace("gitee.com/", "gitee.com.evil.invalid/"),
        source.as_str().replace("xiaotangtoken/", "another-owner/"),
    ] {
        assert!(gitee_raw_url(&Url::parse(&invalid)?).is_none(), "{invalid}");
    }
    let redirect = format!("https://raw.giteeusercontent.com{raw_path}?signature=abc");
    assert_eq!(
        gitee_signed_location(&raw, &redirect),
        Some(Url::parse(&redirect)?)
    );
    for invalid in [
        redirect.replace("https:", "http:"),
        redirect.replace(
            "raw.giteeusercontent.com",
            "raw.giteeusercontent.com.evil.invalid",
        ),
        redirect.replace("raw.giteeusercontent.com", "user@raw.giteeusercontent.com"),
        redirect.replace("preview", "stable"),
        redirect.replace("signature=abc", "token=abc"),
        redirect.replace("signature=abc", "signature="),
        redirect.replace("catalog.json", "avatar-author.png"),
    ] {
        assert!(gitee_signed_location(&raw, &invalid).is_none(), "{invalid}");
    }
    Ok(())
}

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Result<Self> {
        let path = std::env::temp_dir().join(format!(
            "daozai-market-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        ));
        fs::create_dir(&path)?;
        Ok(Self(path))
    }

    fn project(&self, version: &str) -> Result<PathBuf> {
        let path = self.0.join("plugin.json");
        let mut manifest: Value =
            serde_json::from_str(include_str!("examples/payday/plugin.json"))?;
        manifest["version"] = json!(version);
        fs::write(&path, serde_json::to_vec(&manifest)?)?;
        fs::write(
            self.0.join("main.js"),
            include_str!("examples/payday/main.js"),
        )?;
        fs::write(
            self.0.join(COVER_FILE),
            include_bytes!("examples/payday/cover.png"),
        )?;
        Ok(path)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn market_author_template_is_valid_and_preserves_existing_files() -> Result<()> {
    let fixture = Fixture::new()?;
    assert!(create_template(&fixture.0, "../escape", "发薪倒计时", "小明").is_err());
    assert!(create_template(&fixture.0, "xiaoming.payday", " ", "小明").is_err());
    assert!(create_template(&fixture.0, "xiaoming.payday", "发薪倒计时", " ").is_err());
    assert_eq!(fs::read_dir(&fixture.0)?.count(), 0);

    let path = create_template(&fixture.0, "xiaoming.payday", "自定义倒计时", "小明")?;
    assert_eq!(path, fixture.0.join("xiaoming.payday/plugin.json"));
    assert_eq!(validate_project(&path)?, "xiaoming.payday v1.0.0 校验通过");
    let original = fs::read_to_string(&path)?;
    let manifest = Manifest::decode(&original)?;
    assert_eq!(manifest.name, "自定义倒计时");
    assert_eq!(manifest.author, "小明");
    assert_eq!(
        fs::read(path.with_file_name("LICENSE"))?.as_slice(),
        include_bytes!("examples/payday/LICENSE")
    );

    assert!(create_template(&fixture.0, "xiaoming.payday", "另一个插件", "另一位作者").is_err());
    assert_eq!(fs::read_to_string(&path)?, original);
    assert_eq!(fs::read_dir(&fixture.0)?.count(), 1);
    fs::remove_file(path.with_file_name(COVER_FILE))?;
    assert!(validate_project(&path).is_err());
    Ok(())
}

#[test]
fn market_author_packages_and_submits_immutable_versions() -> Result<()> {
    let fixture = Fixture::new()?;
    let catalog_path = fixture.0.join("catalog.json");
    let first = fixture.0.join("first.zip");
    pack(&fixture.project("1.2.9")?, &first)?;
    assert!(pack(&fixture.project("1.2.9")?, &first).is_err());
    submit(&first, &catalog_path)?;

    let catalog = Catalog::open(&catalog_path)?;
    let original = &catalog.entries[0];
    assert!(original.presentation.is_none());
    assert_eq!(catalog.load(original)?.manifest.version, "1.2.9");
    assert_eq!(
        catalog.cover_png(original)?.as_slice(),
        include_bytes!("examples/payday/cover.png")
    );
    let original_path = original.path_in(&catalog.path);
    let original_bytes = fs::read(&original_path)?;
    assert!(submit(&first, &catalog_path).is_err());

    let mut index: Value = serde_json::from_slice(&fs::read(&catalog_path)?)?;
    let presentation = json!({
        "description": "发薪日提醒",
        "updated_on": "2026-11-21",
        "official": true,
        "sample": true,
        "maintainer": "Renly1994",
        "author_avatar": "avatar-renly1994.png",
        "downloads": 22,
        "favorites": 0
    });
    index["plugins"][0]["presentation"] = presentation.clone();
    fs::write(&catalog_path, serde_json::to_vec(&index)?)?;

    let second = fixture.0.join("second.zip");
    pack(&fixture.project("1.2.10")?, &second)?;
    fs::copy(&second, fixture.0.join("plugin-example.payday-1.2.10.zip"))?;
    submit(&second, &catalog_path)?;
    let catalog = Catalog::open(&catalog_path)?;
    assert_eq!(catalog.entries.len(), 1);
    assert_eq!(
        catalog.load(&catalog.entries[0])?.manifest.version,
        "1.2.10"
    );
    assert_eq!(fs::read(&original_path)?, original_bytes);
    assert!(submit(&first, &catalog_path).is_err());

    let entry = &catalog.entries[0];
    assert_eq!(serde_json::to_value(&entry.presentation)?, presentation);
    let mut raw = fs::read(entry.path_in(&catalog.path))?;
    raw[0] ^= 1;
    fs::write(entry.path_in(&catalog.path), raw)?;
    assert!(catalog
        .load(entry)
        .err()
        .expect("损坏的版本包应被拒绝")
        .to_string()
        .contains("SHA-256"));
    assert!(catalog.cover_png(entry).is_err());
    Ok(())
}

fn publish_wireless(root: &Path, version: &str, data_version: &str) -> Result<()> {
    let manifest_path = root.join("plugin.json");
    fs::write(
        root.join(COVER_FILE),
        include_bytes!("examples/payday/cover.png"),
    )?;
    let manifest = json!({
        "id": WIRELESS_PLUGIN_ID,
        "name": "无线设备兼容包",
        "author": "岛仔",
        "version": version,
        "api": 1,
        "runtime": "assets",
        "entry": "wireless-receivers.json",
        "cover": COVER_FILE,
        "owner": "wireless",
        "capabilities": ["wireless.receivers"],
        "dataVersion": 1,
        "permissions": {},
        "settings": []
    });
    let data = json!({
        "id": WIRELESS_PLUGIN_ID,
        "api_version": 1,
        "version": data_version,
        "receivers": [{"vid": "046D", "pid": "C542", "name": "罗技 Nano 接收器"}]
    });
    fs::write(&manifest_path, serde_json::to_vec(&manifest)?)?;
    fs::write(root.join("wireless-receivers.json"), serde_json::to_vec(&data)?)?;
    let bundle = root.join(format!("wireless-{version}.zip"));
    pack(&manifest_path, &bundle)?;
    submit(&bundle, &root.join("catalog.json"))
}

fn apply_wireless_market_action(host: &mut LocalPlugins) -> Result<()> {
    host.market_action(false, WIRELESS_PLUGIN_ID)?;
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while host.market_busy() {
        host.poll_market();
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    Ok(())
}

impl LocalPlugins {
    pub(crate) fn test_market_install_wireless_compat(&mut self) -> Result<()> {
        let root = self.dir.join("test-wireless-market");
        fs::create_dir_all(&root)?;
        publish_wireless(&root, "1.0.0", "1.0.0")?;
        self.set_market_catalog(&root.join("catalog.json"))?;
        apply_wireless_market_action(self)
    }

    pub(crate) fn test_uninstall_wireless_compat(&mut self) -> Result<()> {
        self.uninstall(WIRELESS_PLUGIN_ID, true)
    }
}

#[test]
fn market_installs_wireless_compatibility_as_a_resource_package() -> Result<()> {
    use super::super::market_state::MarketStatus;
    let fixture = Fixture::new()?;
    let root = &fixture.0;
    let id = WIRELESS_PLUGIN_ID;
    let catalog_path = root.join("catalog.json");

    assert!(publish_wireless(root, "1.1.0", "1.0.0").is_err());
    publish_wireless(root, "1.0.0", "1.0.0")?;
    let catalog = Catalog::open(&catalog_path)?;
    let entry = &catalog.entries[0];
    assert_eq!(entry.owner(), Some("wireless"));
    assert!(catalog.load(entry)?.manifest.is_wireless());

    let storage = fixture.0.join("installed");
    let mut host = LocalPlugins::open(storage.clone());
    let wireless = host.wireless_store();
    assert!(wireless.package().is_none());
    assert!(host.install(&root.join("plugin.json")).is_err());
    host.set_market_catalog(&catalog_path)?;
    apply_wireless_market_action(&mut host)?;
    assert_eq!(host.market_status(&catalog, entry), MarketStatus::Installed);
    assert!(!host.records[id].enabled);
    assert!(wireless.package().is_none());
    host.enable(id, true)?;
    assert_eq!(wireless.package().unwrap().version(), "1.0.0");
    host.enable(id, false)?;
    assert!(wireless.package().is_none());
    host.enable(id, true)?;

    publish_wireless(root, "1.1.0", "1.1.0")?;
    host.refresh_market_catalog()?;
    apply_wireless_market_action(&mut host)?;
    assert_eq!(wireless.package().unwrap().version(), "1.1.0");
    let mut reopened = LocalPlugins::open(storage);
    let record = &reopened.records[id];
    assert!(record.enabled);
    assert_eq!(record.package.as_ref().unwrap().manifest.version, "1.1.0");
    let wireless = reopened.wireless_store();
    assert_eq!(wireless.package().unwrap().version(), "1.1.0");
    reopened.uninstall(id, false)?;
    assert!(!reopened.records.contains_key(id));
    assert!(wireless.package().is_none());
    Ok(())
}

#[test]
fn market_rejects_invalid_archives_and_catalogs() -> Result<()> {
    let fixture = Fixture::new()?;
    let manifest = fixture.project("1.0.0")?;
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file("plugin.json", SimpleFileOptions::default())?;
    zip.write_all(&fs::read(&manifest)?)?;
    zip.add_symlink("main.js", "outside.js", SimpleFileOptions::default())?;
    zip.start_file(COVER_FILE, SimpleFileOptions::default())?;
    zip.write_all(include_bytes!("examples/payday/cover.png"))?;
    assert!(decode_zip(&zip.finish()?.into_inner()).is_err());

    let archive = |add_cover: bool, link: bool| -> Result<Vec<u8>> {
        let options = SimpleFileOptions::default();
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        zip.start_file("plugin.json", options)?;
        zip.write_all(&fs::read(&manifest)?)?;
        zip.start_file("main.js", options)?;
        zip.write_all(include_bytes!("examples/payday/main.js"))?;
        if link {
            zip.add_symlink(COVER_FILE, "../outside.png", options)?;
        } else if add_cover {
            zip.start_file(COVER_FILE, options)?;
            zip.write_all(b"broken png")?;
        }
        Ok(zip.finish()?.into_inner())
    };
    for (add_cover, link) in [(false, false), (true, false), (true, true)] {
        assert!(decode_zip(&archive(add_cover, link)?).is_err());
    }

    let bundle = fixture.0.join("package.zip");
    let catalog = fixture.0.join("catalog.json");
    pack(&fixture.project("1.0.0")?, &bundle)?;
    submit(&bundle, &catalog)?;
    let mut data: Value = serde_json::from_slice(&fs::read(&catalog)?)?;
    let entry = data["plugins"][0].clone();
    data["plugins"] = json!([entry.clone(), entry]);
    fs::write(&catalog, serde_json::to_vec(&data)?)?;
    assert!(Catalog::open(&catalog).is_err());
    data["plugins"] = json!([entry]);
    for presentation in [
        json!({"updated_on": "2026-13-01"}),
        json!({"author_avatar": "https://example.invalid/avatar.png"}),
        json!({"author_avatar": "../avatar-renly1994.png"}),
        json!({"maintainer": "@island"}),
        json!({"downloads": -1}),
    ] {
        data["plugins"][0]["presentation"] = presentation;
        fs::write(&catalog, serde_json::to_vec(&data)?)?;
        assert!(Catalog::open(&catalog).is_err());
    }
    assert!(compare_versions("1.02.0", "1.2.0").is_none());
    assert!(compare_versions("1.4294967296.0", "1.2.0").is_none());
    Ok(())
}

#[test]
fn market_revocation_disables_installed_version_and_offers_newer_package() -> Result<()> {
    use super::super::market_state::MarketStatus;

    let fixture = Fixture::new()?;
    let id = "example.payday";
    let catalog_path = fixture.0.join("catalog.json");
    let first = fixture.0.join("first.zip");
    pack(&fixture.project("1.0.1")?, &first)?;
    submit(&first, &catalog_path)?;

    let storage = fixture.0.join("installed");
    let mut host = LocalPlugins::open(storage.clone());
    host.set_market_catalog(&catalog_path)?;
    host.market_action(false, id)?;
    wait_for_catalog_action(&mut host);
    host.enable(id, true)?;
    let mut index: Value = serde_json::from_slice(&fs::read(&catalog_path)?)?;
    index["plugins"][0]["revoked"] = json!(["invalid-version"]);
    fs::write(&catalog_path, serde_json::to_vec(&index)?)?;
    assert!(host.refresh_market_catalog().is_err());
    assert!(host.records[id].enabled);
    index["plugins"][0]["revoked"] = json!(["1.0.1"]);
    fs::write(&catalog_path, serde_json::to_vec(&index)?)?;
    host.refresh_market_catalog()?;
    assert!(!host.records[id].enabled);
    assert!(host.records[id].market_revoked);
    assert!(host.enable(id, true).is_err());
    assert!(host.market_action(false, id).is_err());
    let catalog = Catalog::open(&catalog_path)?;
    assert!(catalog.load(&catalog.entries[0]).is_err());
    let mut clean = LocalPlugins::open(fixture.0.join("clean"));
    clean.set_market_catalog(&catalog_path)?;
    assert!(clean.market_action(false, id).is_err());
    drop(host);

    let mut host = LocalPlugins::open(storage);
    assert!(host.records[id].market_revoked);
    let next = fixture.0.join("next.zip");
    pack(&fixture.project("1.0.2")?, &next)?;
    submit(&next, &catalog_path)?;
    let catalog = Catalog::open(&catalog_path)?;
    assert!(catalog.entries[0].revokes("1.0.1"));
    host.refresh_market_catalog()?;
    assert_eq!(host.market_status(&catalog, &catalog.entries[0]), MarketStatus::Update);
    host.market_action(false, id)?;
    wait_for_catalog_action(&mut host);
    assert!(!host.records[id].market_revoked);
    assert!(!host.records[id].enabled);
    host.enable(id, true)?;
    Ok(())
}

fn wait_for_catalog_action(host: &mut LocalPlugins) {
    let deadline = std::time::Instant::now() + Duration::from_secs(6);
    while host.market_busy() {
        assert!(std::time::Instant::now() < deadline, "等待集市操作超时");
        host.poll_market();
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn update_removes_https_grant_when_host_is_removed() -> Result<()> {
    let fixture = Fixture::new()?;
    let mut manifest: Value =
        serde_json::from_str(include_str!("examples/https-repository/plugin.json"))?;
    let path = fixture.0.join("plugin.json");
    fs::write(fixture.0.join("main.js"), include_str!("examples/https-repository/main.js"))?;
    fs::write(fixture.0.join(COVER_FILE), include_bytes!("examples/https-repository/cover.png"))?;
    fs::write(&path, serde_json::to_vec(&manifest)?)?;
    let catalog = fixture.0.join("catalog.json");
    let first = fixture.0.join("first.zip");
    pack(&path, &first)?;
    submit(&first, &catalog)?;
    let mut host = LocalPlugins::open(fixture.0.join("installed"));
    let id = "example.https-repository";
    host.set_market_catalog(&catalog)?;
    host.market_action(false, id)?;
    wait_for_catalog_action(&mut host);

    let grant = host.records[id].network().unwrap().grant("https://api.github.com/repos/rust-lang/rust")?;
    host.set_source_grant(id, Some(grant.clone()))?;
    assert_eq!(host.source_grant(id), Some(&grant));
    manifest["version"] = json!("1.0.1");
    manifest["network"] = json!({"hosts": ["example.org"]});
    fs::write(&path, serde_json::to_vec(&manifest)?)?;
    let next = fixture.0.join("next.zip");
    pack(&path, &next)?;
    submit(&next, &catalog)?;
    host.refresh_market_catalog()?;
    host.market_action(false, id)?;
    wait_for_catalog_action(&mut host);
    assert_eq!(host.records[id].package.as_ref().unwrap().manifest.version, "1.0.1");
    assert!(host.source_grant(id).is_none());
    assert!(host.records[id].source_grant.is_none());
    Ok(())
}
