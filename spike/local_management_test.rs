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
            let id = match label {
                "启用" | "停用" => ["example.payday", "example.material"]
                    .into_iter()
                    .map(|plugin| egui::Id::new(("settings-plugin-switch", plugin)))
                    .find(|id| ctx.read_response(*id).is_some())?,
                "刷新目录" => egui::Id::new("plugin-market-refresh"),
                _ => return None,
            };
            ctx.read_response(id).map(|response| response.rect.center())
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
                true,
                #[cfg(all(windows, feature = "sdk-material"))]
                None,
            ) == Some(4);
        });
    };
    click(&mut host, "启用");
    assert!(host.records["example.payday"].enabled);
    click(&mut host, "设置 › 待办与置物 ›");
    assert_eq!(host.focus.as_deref(), Some("example.payday"));
    click(&mut host, "停用");
    assert!(!host.records["example.payday"].enabled);
    click(&mut host, "···");
    click(&mut host, "卸载…");
    click(&mut host, "取消");
    assert!(host.records["example.payday"].package.is_some());
    click(&mut host, "···");
    click(&mut host, "卸载…");
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
    let settings = |host: &mut LocalPlugins, owner| {
        frame(&ctx, vec![], |ui| host.settings(ui, "tasks", owner))
    };
    let manage = |host: &mut LocalPlugins, ui: &mut egui::Ui, owner| {
        host.manage(
            ui,
            owner,
            true,
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
    assert!(has_text(&paused, "页面管理 › 待办与置物 ›"));
    assert!(!has_text(&paused, "设置 › 待办与置物 ›"));
    let mut jump = None;
    click(&ctx, "页面管理 › 待办与置物 ›", |ui| {
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

#[test]
fn invalid_record_can_be_removed_from_the_installed_list() -> Result<()> {
    let dir = std::env::temp_dir().join(format!(
        "daozai-manage-invalid-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    ));
    let mut host = LocalPlugins::open(dir.clone());
    host.install(&Path::new(env!("CARGO_MANIFEST_DIR")).join("spike/examples/payday/plugin.json"))?;
    drop(host);
    let record = dir.join("local-example.payday.json");
    fs::write(&record, "{}")?;
    let mut host = LocalPlugins::open(dir.clone());
    assert!(host.invalid.contains("example.payday"));
    let ctx = egui::Context::default();
    crate::theme::load_fonts(&ctx);
    let mut draw = |ui: &mut egui::Ui| {
        host.manage(
            ui,
            true,
            true,
            #[cfg(all(windows, feature = "sdk-material"))]
            None,
        );
    };
    assert!(has_text(&frame(&ctx, vec![], &mut draw), "需要处理"));
    click(&ctx, "移除记录", &mut draw);
    assert!(host.invalid.is_empty());
    assert!(!record.exists());
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
        host.header_tabs(ui);
        host.manage(
            ui,
            true,
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
    assert!(ctx
        .read_response(egui::Id::new("plugin-market-refresh"))
        .is_some());
    assert!(!has_text(&official, "发薪倒计时"));
    assert!(!has_text(&official, "导入测试集市目录…"));
    click_button(&mut host, "已安装");
    click_button(&mut host, "开发者工具");
    let developers = frame(&ctx, vec![], |ui| draw(&mut host, ui));
    assert!(has_text(&developers, "导入测试集市目录…"));
    click_button(&mut host, "关闭");
    click_button(&mut host, "集市");
    click_button(&mut host, "本地测试");
    assert!(has_text(
        &frame(&ctx, vec![], |ui| draw(&mut host, ui)),
        "本地测试集市"
    ));
    click_button(&mut host, "安装");
    finish(&mut host);
    assert!(!host.records["example.payday"].enabled);
    click_button(&mut host, "已安装");
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
    click_button(&mut host, "集市");
    click_button(&mut host, "本地测试");
    click_button(&mut host, "刷新目录");
    let available = frame(&ctx, vec![], |ui| draw(&mut host, ui));
    assert!(has_text(&available, "更新"));
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

#[cfg(windows)]
#[test]
fn official_market_only_requests_uncached_data_or_explicit_refresh() -> Result<()> {
    use base64::{engine::general_purpose::STANDARD, Engine};
    use market_metrics::tests::next_request;
    use std::{io::Write, net::TcpListener, sync::mpsc, thread};

    const ID: &str = "example.payday";
    const CONTENTS: &str = "/contents/preview/";
    const ZIP: &str = "plugin-example.payday-1.0.0.zip";
    let dir = std::env::temp_dir().join(format!(
        "daozai-refresh-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    ));
    fs::create_dir_all(&dir)?;
    let sample = Path::new(env!("CARGO_MANIFEST_DIR")).join("spike/examples/payday/plugin.json");
    let bundle = dir.join("sample.zip");
    let index_file = dir.join("catalog.json");
    market::pack(&sample, &bundle)?;
    market::submit(&bundle, &index_file)?;
    let contents = |name: &str| -> Result<Vec<u8>> {
        let bytes = fs::read(dir.join(name))?;
        Ok(serde_json::to_vec(&json!({
            "type": "file", "path": format!("preview/{name}"), "encoding": "base64",
            "size": bytes.len(), "content": STANDARD.encode(&bytes)
        }))?)
    };
    let index_body = contents("catalog.json")?;
    let zip_body = contents(ZIP)?;
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let addr = listener.local_addr()?;
    listener.set_nonblocking(true)?;
    let (log_request, requests) = mpsc::channel();
    let (stop, stopped) = mpsc::channel::<()>();
    let server = thread::spawn(move || {
        let mut downloads = 0;
        let mut favorites = 0;
        while let Err(mpsc::TryRecvError::Empty) = stopped.try_recv() {
            let (mut stream, _) = match listener.accept() {
                Ok(peer) => peer,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(1));
                    continue;
                }
                Err(error) => panic!("{error}"),
            };
            stream.set_nonblocking(false).unwrap();
            let (line, data) = next_request(&mut stream);
            let line = line.trim_end();
            let snapshot = |downloads, favorites| {
                serde_json::to_vec(&json!({"plugins": {
                    ID: {"downloads": downloads, "favorites": favorites}
                }}))
                .unwrap()
            };
            let (kind, body) = match line {
                "GET /contents/preview/catalog.json?ref=master HTTP/1.1" => {
                    ("目录 GET", index_body.clone())
                }
                "GET /contents/preview/plugin-example.payday-1.0.0.zip?ref=master HTTP/1.1" => {
                    ("ZIP GET", zip_body.clone())
                }
                "GET /v1/stats?channel=preview HTTP/1.1" => {
                    ("统计 GET", snapshot(downloads, favorites))
                }
                "POST /v1/events HTTP/1.1" => {
                    assert_eq!(data["channel"], "preview");
                    let events = data["events"].as_array().unwrap();
                    assert_eq!(events.len(), 1);
                    let event = &events[0];
                    assert_eq!(event["plugin_id"], ID);
                    let kind = match event["kind"].as_str().unwrap() {
                        "download" => {
                            assert_eq!(event["version"], "1.0.0");
                            downloads += 1;
                            "下载 POST"
                        }
                        "favorite" => {
                            favorites = u64::from(event["saved"].as_bool().unwrap());
                            if favorites == 1 {
                                "收藏 POST"
                            } else {
                                "取消收藏 POST"
                            }
                        }
                        kind => panic!("未预期的事件 {kind}"),
                    };
                    (kind, snapshot(downloads, favorites))
                }
                line => panic!("未预期的请求 {line}"),
            };
            log_request.send(kind).unwrap();
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .unwrap();
            stream.write_all(&body).unwrap();
        }
    });

    let storage = dir.join("installed");
    let open = || -> Result<LocalPlugins> {
        let mut host = LocalPlugins::open(storage.clone());
        host.set_official_test_index(url::Url::parse(&format!(
            "http://{addr}{CONTENTS}catalog.json?ref=master"
        ))?);
        host.market_metrics = market_metrics::MarketMetrics::for_test(
            &storage,
            url::Url::parse(&format!("http://{addr}"))?,
        );
        Ok(host)
    };
    let ctx = egui::Context::default();
    crate::theme::load_fonts(&ctx);
    ctx.style_mut(|style| style.animation_time = 0.);
    let draw = |host: &mut LocalPlugins, ui: &mut egui::Ui| {
        host.header_tabs(ui);
        let _ = host.manage(
            ui,
            true,
            true,
            #[cfg(feature = "sdk-material")]
            None,
        );
    };
    let click_button = |host: &mut LocalPlugins, label| click(&ctx, label, |ui| draw(host, ui));
    let finish = |host: &mut LocalPlugins| {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            frame(&ctx, vec![], |ui| draw(host, ui));
            if !host.market_pending() {
                break;
            }
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(1));
        }
    };
    let assert_requests = |phase, expected: &[&str]| {
        let mut actual: Vec<_> = requests.try_iter().collect();
        let mut expected = expected.to_vec();
        actual.sort_unstable();
        expected.sort_unstable();
        assert_eq!(actual, expected, "{phase}");
        println!("{phase} {actual:?}");
    };

    let mut host = open()?;
    host.set_market_catalog(&index_file)?;
    finish(&mut host);
    assert_requests("首次打开", &["目录 GET", "统计 GET", "ZIP GET"]);
    assert!(
        host.market_stats_repaint_delay().is_none(),
        "空闲集市无需定时刷新"
    );
    click_button(&mut host, "收藏 0");
    click_button(&mut host, "全部");
    click_button(&mut host, "已安装");
    click_button(&mut host, "去插件集市");
    click_button(&mut host, "本地测试");
    click_button(&mut host, "官方");
    click_button(&mut host, "发薪倒计时");
    let detail = egui::Id::new("plugin-market-details");
    assert_eq!(
        ctx.data(|data| data.get_temp::<String>(detail)).as_deref(),
        Some(ID)
    );
    let _ = frame(
        &ctx,
        vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        }],
        |ui| draw(&mut host, ui),
    );
    assert!(ctx.data(|data| data.get_temp::<String>(detail)).is_none());
    let _ = frame(&ctx, vec![], |ui| {
        ui.label("其他设置");
    });
    finish(&mut host);
    assert_requests("切标签、从详情返回及重新打开设置", &[]);

    click_button(&mut host, "刷新目录");
    finish(&mut host);
    assert_requests("手动刷新", &["目录 GET", "统计 GET"]);
    click_button(&mut host, "安装");
    finish(&mut host);
    assert_eq!(
        host.records[ID].package.as_ref().unwrap().manifest.version,
        "1.0.0"
    );
    assert_eq!(host.market_counts(ID).unwrap().downloads, 1);
    assert_requests("安装", &["ZIP GET", "下载 POST"]);
    let catalog = host.market_catalog(true).unwrap();
    let entry = &catalog.entries[0];
    host.set_favorite(entry, true)?;
    finish(&mut host);
    assert_eq!(host.market_counts(ID).unwrap().favorites, 1);
    assert_requests("收藏", &["收藏 POST"]);
    host.remove_favorite(ID)?;
    finish(&mut host);
    assert_eq!(host.market_counts(ID).unwrap().favorites, 0);
    assert_requests("取消收藏", &["取消收藏 POST"]);

    drop(host);
    let mut host = open()?;
    host.ensure_official_catalog();
    assert_eq!(host.market_counts(ID).unwrap().downloads, 1);
    assert!(host.market_catalog(true).is_some());
    assert!(!host.market_pending());
    assert_requests("重启后恢复目录和统计", &[]);
    stop.send(())?;
    server.join().unwrap();
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
            jump = host.manage(ui, true, true, None).or(jump)
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
    for expected in ["0.50px", "0.500", "插件"] {
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
