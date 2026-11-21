use super::*;
use eframe::egui;

fn frame(
    ctx: &egui::Context,
    events: Vec<egui::Event>,
    mut draw: impl FnMut(&mut egui::Ui),
) -> egui::FullOutput {
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(500., 550.),
            )),
            events,
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, &mut draw);
        },
    )
}

fn click(ctx: &egui::Context, label: &str, mut draw: impl FnMut(&mut egui::Ui)) {
    let _ = frame(ctx, vec![], &mut draw);
    let out = frame(ctx, vec![], &mut draw);
    let pos = out
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.text() == label => {
                Some(text.visual_bounding_rect().center())
            }
            _ => None,
        })
        .or_else(|| {
            if matches!(label, "启用" | "停用") {
                let id = egui::Id::new(("settings-plugin-switch", "example.payday"));
                ctx.read_response(id).map(|response| response.rect.center())
            } else {
                None
            }
        })
        .unwrap_or_else(|| panic!("找不到按钮 {label}"));
    click_at(ctx, pos, draw);
}

fn click_at(ctx: &egui::Context, pos: egui::Pos2, mut draw: impl FnMut(&mut egui::Ui)) {
    for pressed in [true, false] {
        let _ = frame(
            ctx,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                },
            ],
            &mut draw,
        );
    }
}

fn has_text(output: &egui::FullOutput, label: &str) -> bool {
    output
        .shapes
        .iter()
        .any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.text() == label))
}

#[test]
fn management_buttons_jump_disable_and_confirm_uninstall() -> Result<()> {
    let dir = std::env::temp_dir().join(format!(
        "daozai-manage-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    ));
    let mut host = LocalPlugins::open(dir.clone());
    host.install(&Path::new(env!("CARGO_MANIFEST_DIR")).join("spike/examples/payday/plugin.json"))?;
    let mut legacy = host.records["example.payday"].clone();
    let package = legacy.package.as_mut().unwrap();
    package.manifest.cover = None;
    package.cover = None;
    host.put("example.payday", legacy)?;
    drop(host);
    let mut host = LocalPlugins::open(dir.clone());
    let ctx = egui::Context::default();
    crate::theme::load_fonts(&ctx);
    let first = frame(&ctx, vec![], |ui| {
        host.manage(
            ui,
            true,
            #[cfg(all(windows, feature = "sdk-material"))]
            None,
        );
    });
    assert!(has_text(&first, "暂无封面"));
    let mut jumped = false;
    let mut click = |host: &mut LocalPlugins, label: &str| {
        click(&ctx, label, |ui| {
            jumped |= host.manage(
                ui,
                true,
                #[cfg(all(windows, feature = "sdk-material"))]
                None,
            ) == Some(4);
        });
    };
    click(&mut host, "启用");
    assert!(host.records["example.payday"].enabled);
    click(&mut host, "前往设置");
    assert_eq!(host.focus.as_deref(), Some("example.payday"));
    click(&mut host, "停用");
    assert!(!host.records["example.payday"].enabled);
    click(&mut host, "卸载");
    click(&mut host, "取消");
    assert!(host.records["example.payday"].package.is_some());
    click(&mut host, "卸载");
    click(&mut host, "删除数据");
    assert!(!host.records.contains_key("example.payday"));
    assert!(jumped);
    fs::remove_dir(dir)?;
    Ok(())
}

#[test]
fn management_module_pause_and_plugin_switch_keep_their_own_choices() -> Result<()> {
    let dir = std::env::temp_dir().join(format!(
        "daozai-manage-switches-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    ));
    let mut host = LocalPlugins::open(dir.clone());
    host.install(&Path::new(env!("CARGO_MANIFEST_DIR")).join("spike/examples/payday/plugin.json"))?;
    let ctx = egui::Context::default();
    crate::theme::load_fonts(&ctx);
    let settings =
        |host: &mut LocalPlugins, owner| frame(&ctx, vec![], |ui| host.settings(ui, owner));
    let manage = |host: &mut LocalPlugins, ui: &mut egui::Ui, owner| {
        host.manage(
            ui,
            owner,
            #[cfg(all(windows, feature = "sdk-material"))]
            None,
        )
    };
    assert!(!has_text(&settings(&mut host, true), "倒计时名称"));
    click(&ctx, "启用", |ui| {
        let _ = manage(&mut host, ui, true);
    });
    assert!(has_text(&settings(&mut host, true), "倒计时名称"));
    assert!(!has_text(&settings(&mut host, false), "倒计时名称"));
    let paused = frame(&ctx, vec![], |ui| {
        let _ = manage(&mut host, ui, false);
    });
    assert!(has_text(&paused, "已开启 · 模块已暂停"));
    assert!(!has_text(&paused, "前往设置"));
    let mut jump = None;
    click(&ctx, "前往页面管理", |ui| {
        jump = manage(&mut host, ui, false).or(jump);
    });
    assert_eq!(jump, Some(crate::extras::SETTINGS_TAB_PAGES));

    click(&ctx, "停用", |ui| {
        let _ = manage(&mut host, ui, false);
    });
    assert!(!has_text(&settings(&mut host, true), "倒计时名称"));
    click(&ctx, "启用", |ui| {
        let _ = manage(&mut host, ui, false);
    });
    assert!(!has_text(&settings(&mut host, false), "倒计时名称"));
    assert!(has_text(&settings(&mut host, true), "倒计时名称"));

    host.cards
        .insert("example.payday".into(), "发薪倒计时".into());
    let mut opened = false;
    let mut draw = |ui: &mut egui::Ui| {
        let card = crate::paint::Card::new(ui, egui::Pos2::ZERO, ui.max_rect());
        opened |= host.card(&card);
    };
    let _ = frame(&ctx, vec![], &mut draw);
    let _ = frame(&ctx, vec![], &mut draw);
    click_at(&ctx, egui::pos2(363., 145.), &mut draw);
    assert!(opened);
    assert_eq!(host.focus.as_deref(), Some("example.payday"));
    fs::remove_dir_all(dir)?;
    Ok(())
}

#[cfg(windows)]
#[test]
fn market_buttons_install_and_update_an_existing_sample() -> Result<()> {
    let dir = std::env::temp_dir().join(format!(
        "daozai-manage-market-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    ));
    let files = dir.join("author");
    let published = dir.join("market");
    fs::create_dir_all(&files)?;
    fs::create_dir(&published)?;
    let sample = Path::new(env!("CARGO_MANIFEST_DIR")).join("spike/examples/payday/plugin.json");
    let catalog = published.join("catalog.json");
    let first = files.join("first.zip");
    market::pack(&sample, &first)?;
    market::submit(&first, &catalog)?;
    let mut host = LocalPlugins::open(dir.join("installed"));
    host.set_market_catalog(&catalog)?;

    let ctx = egui::Context::default();
    crate::theme::load_fonts(&ctx);
    ctx.style_mut(|style| style.animation_time = 0.);
    let draw = |host: &mut LocalPlugins, ui: &mut egui::Ui| {
        host.manage(
            ui,
            true,
            #[cfg(all(windows, feature = "sdk-material"))]
            None,
        );
    };
    let click_button = |host: &mut LocalPlugins, label| click(&ctx, label, |ui| draw(host, ui));
    let finish = |host: &mut LocalPlugins| {
        let deadline = Instant::now() + Duration::from_secs(3);
        while host.market_busy() && Instant::now() < deadline {
            frame(&ctx, vec![], |ui| draw(host, ui));
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(!host.market_busy(), "测试集市操作未完成");
    };
    let official = frame(&ctx, vec![], |ui| draw(&mut host, ui));
    assert!(has_text(&official, "刷新目录"));
    assert!(!has_text(&official, "发薪倒计时"));
    assert!(!has_text(&official, "导入测试集市目录…"));
    click_button(&mut host, "我的插件 · 0");
    click_button(&mut host, "开发者模式");
    let developers = frame(&ctx, vec![], |ui| draw(&mut host, ui));
    assert!(has_text(&developers, "导入测试集市目录…"));
    click_button(&mut host, "打开测试集市");
    assert!(has_text(
        &frame(&ctx, vec![], |ui| draw(&mut host, ui)),
        "本地测试集市"
    ));
    click_button(&mut host, "安装");
    finish(&mut host);
    assert!(!host.records["example.payday"].enabled);
    click_button(&mut host, "返回我的插件");
    click_button(&mut host, "启用");
    assert!(host.records["example.payday"].enabled);

    let mut manifest: Value = serde_json::from_slice(&fs::read(&sample)?)?;
    manifest["version"] = json!("1.1.0");
    let updated = files.join("plugin.json");
    fs::write(&updated, serde_json::to_vec(&manifest)?)?;
    fs::copy(sample.with_file_name("main.js"), files.join("main.js"))?;
    fs::copy(sample.with_file_name("cover.png"), files.join("cover.png"))?;
    let second = files.join("second.zip");
    market::pack(&updated, &second)?;
    market::submit(&second, &catalog)?;
    click_button(&mut host, "打开测试集市");
    click_button(&mut host, "刷新目录");
    let available = frame(&ctx, vec![], |ui| draw(&mut host, ui));
    assert!(has_text(&available, "已安装 v1.0.0"));
    click_button(&mut host, "更新");
    finish(&mut host);

    let record = &host.records["example.payday"];
    assert_eq!(record.package.as_ref().unwrap().manifest.version, "1.1.0");
    assert!(record.enabled);
    assert!(has_text(
        &frame(&ctx, vec![], |ui| draw(&mut host, ui)),
        "已安装"
    ));
    fs::remove_dir_all(dir)?;
    Ok(())
}

#[cfg(all(windows, feature = "sdk-material"))]
#[test]
fn material_management_waits_for_permission_and_offers_to_enable_compatibility() -> Result<()> {
    let dir = std::env::temp_dir().join(format!(
        "daozai-manage-material-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    ));
    let package = dir.join("package");
    fs::create_dir_all(&package)?;
    let manifest = package.join("plugin.json");
    fs::write(
        package.join("cover.png"),
        include_bytes!("examples/payday/cover.png"),
    )?;
    fs::write(
        package.join("material.wgsl"),
        "fn material(i: MaterialIn) -> vec4<f32> { return vec4<f32>(i.px, 0.0, 1.0); }",
    )?;
    fs::write(
        &manifest,
        serde_json::to_vec(&json!({
            "id": "example.material",
            "name": "测试材质",
            "author": "测试",
            "version": "1.0.0",
            "api": 1,
            "runtime": "material",
            "entry": "material.wgsl",
            "cover": "cover.png",
            "owner": "appearance",
            "capabilities": ["appearance.material", "settings"],
            "dataVersion": 1,
            "permissions": {"screen.backdrop": true},
            "material": {"controls": true, "spring": {"stiffness": 1.0, "damping": 0.5}},
            "settings": [
                {"key": "tint", "label": "着色量", "type": "number", "min": 0, "max": 1, "step": 0.1, "default": "0.5", "unit": "px", "precision": 2},
                {"key": "blur", "label": "模糊", "type": "number", "min": 0, "max": 1, "step": 0.005, "default": "0.5"},
                {"key": "strength", "label": "强度", "type": "number", "min": 0, "max": 1, "step": 0.05, "default": "0.54", "precision": 0}
            ]
        }))?,
    )?;
    let mut host = LocalPlugins::open(dir.join("installed"));
    host.install(&manifest)?;
    let ctx = egui::Context::default();
    crate::theme::load_fonts(&ctx);
    let manage_click = |host: &mut LocalPlugins, label: &str| {
        let mut jump = None;
        click(&ctx, label, |ui| {
            jump = host.manage(ui, true, None).or(jump)
        });
        jump
    };

    assert!(manage_click(&mut host, "启用").is_none());
    assert!(host.selected_material_id().is_none());
    assert!(!host.material_permission_granted("example.material"));
    assert!(manage_click(&mut host, "取消").is_none());
    assert!(host.selected_material_id().is_none());
    assert!(manage_click(&mut host, "启用").is_none());
    assert_eq!(
        manage_click(&mut host, "允许并启用"),
        Some(crate::extras::SETTINGS_TAB_APPEARANCE)
    );
    assert_eq!(host.selected_material_id(), Some("example.material"));

    let mut restart = false;
    click(&ctx, "开启并重启", |ui| {
        restart |= host.material_settings(ui, None, false)
    });
    assert!(restart);
    assert_eq!(host.selected_material_id(), Some("example.material"));
    let settings = frame(&ctx, vec![], |ui| {
        host.material_settings(ui, None, false);
    });
    for expected in ["0.50px", "0.500"] {
        assert!(
            settings.shapes.iter().any(|shape| {
                matches!(&shape.shape, egui::Shape::Text(text) if text.galley.text() == expected)
            }),
            "{expected}"
        );
    }
    assert_eq!(host.records["example.material"].values["strength"], "0.54");
    fs::remove_dir_all(dir)?;
    Ok(())
}
