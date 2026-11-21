use super::*;
use serde_json::json;

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
fn market_author_packages_and_submits_immutable_versions() -> Result<()> {
    let fixture = Fixture::new()?;
    let catalog_path = fixture.0.join("catalog.json");
    let first = fixture.0.join("first.zip");
    pack(&fixture.project("1.2.9")?, &first)?;
    assert!(pack(&fixture.project("1.2.9")?, &first).is_err());
    submit(&first, &catalog_path)?;

    let catalog = Catalog::open(&catalog_path)?;
    let original = &catalog.entries[0];
    assert_eq!(catalog.load(original)?.manifest.version, "1.2.9");
    assert_eq!(
        catalog.cover_png(original)?.as_slice(),
        include_bytes!("examples/payday/cover.png")
    );
    let original_path = original.path_in(&catalog.path);
    let original_bytes = fs::read(&original_path)?;
    assert!(submit(&first, &catalog_path).is_err());

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
    assert!(compare_versions("1.02.0", "1.2.0").is_none());
    assert!(compare_versions("1.4294967296.0", "1.2.0").is_none());
    Ok(())
}
