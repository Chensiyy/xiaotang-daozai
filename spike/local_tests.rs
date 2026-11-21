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
            "runtime": "material", "entry": "shaders/material.wgsl", "owner": "appearance",
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
