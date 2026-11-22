use crate::extras::{
    settings_apply_style, settings_card, settings_hint, settings_modal_frame, settings_modal_title,
};
use crate::paint;
use crate::plugin_quota_host::{PluginQuotaHost, PluginQuotaStatus, SourceGrant, SourceKind};
use crate::settings_theme::color;
use eframe::egui::{self, Color32};
use std::path::PathBuf;

#[derive(Clone, Default)]
struct Interaction {
    dialog: Option<Dialog>,
    error: Option<String>,
}

#[derive(Clone)]
enum Dialog {
    Authorize { id: String, address: String },
    Uninstall { id: String },
}

enum Action {
    Install,
    Enable(String, bool),
    Open(Dialog),
}

pub(crate) fn settings(host: &mut PluginQuotaHost, ui: &mut egui::Ui) {
    let key = egui::Id::new("local-quota-plugin-settings");
    let mut state = ui
        .data(|data| data.get_temp::<Interaction>(key))
        .unwrap_or_default();
    if !ui.is_enabled() {
        state.dialog = None;
        state.error = None;
    }
    let mut action = None;
    ui.add_space(8.0);
    settings_card(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("本地额度插件").strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_enabled(cfg!(windows), egui::Button::new("安装插件…"))
                    .clicked()
                {
                    action = Some(Action::Install);
                }
            });
        });
        for record in host.records() {
            ui.separator();
            ui.push_id(record.id, |ui| {
                let status = match record.status {
                    PluginQuotaStatus::Disabled => "已停用",
                    PluginQuotaStatus::Waiting => "等待刷新",
                    PluginQuotaStatus::Unconfigured => "等待数据源授权",
                    PluginQuotaStatus::Offline => "数据源暂不可用",
                    PluginQuotaStatus::Ready => "已接入额度面板",
                    PluginQuotaStatus::Error => "插件未能提供有效额度",
                };
                if plugin_setting_switch_row(ui, record.name, status, record.enabled) {
                    action = Some(Action::Enable(record.id.to_owned(), !record.enabled));
                }
                ui.horizontal_wrapped(|ui| {
                    if record.source_kind.is_some() {
                        let label = if record.source_grant.is_some() {
                            "管理授权…"
                        } else {
                            "授权数据源…"
                        };
                        if ui.button(label).clicked() {
                            let address = match record.source_grant {
                                Some(SourceGrant::Loopback(url)) => url.clone(),
                                _ => String::new(),
                            };
                            action = Some(Action::Open(Dialog::Authorize {
                                id: record.id.to_owned(),
                                address,
                            }));
                        }
                    }
                    if ui.button("卸载…").clicked() {
                        action = Some(Action::Open(Dialog::Uninstall {
                            id: record.id.to_owned(),
                        }));
                    }
                });
            });
        }
        if state.dialog.is_none() {
            show_error(ui, state.error.as_deref());
        }
    });
    if let Some(action) = action {
        state.error = match action {
            Action::Install => pick_file("选择额度插件的 plugin.json")
                .and_then(|path| path.map_or(Ok(()), |path| host.install(&path).map(|_| ())))
                .err(),
            Action::Enable(id, enabled) => host.enable(&id, enabled).err(),
            Action::Open(dialog) => {
                state.dialog = Some(dialog);
                None
            }
        };
        ui.ctx().request_repaint_of(egui::ViewportId::ROOT);
    }
    let ctx = ui.ctx().clone();
    let close = match state.dialog.as_mut() {
        Some(Dialog::Authorize { id, address }) => {
            authorize(host, &ctx, id, address, &mut state.error)
        }
        Some(Dialog::Uninstall { id }) => uninstall(host, &ctx, id, &mut state.error),
        None => false,
    };
    if close {
        state.dialog = None;
        state.error = None;
    }
    ui.data_mut(|data| data.insert_temp(key, state));
}

fn plugin_setting_switch_row(ui: &mut egui::Ui, title: &str, description: &str, on: bool) -> bool {
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        let clicked = plugin_switch(ui, title, on);
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), 0.0),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                plugin_title(ui, title);
                settings_hint(ui, description);
            },
        );
        clicked
    })
    .inner
}

fn plugin_title(ui: &mut egui::Ui, title: &str) {
    let fg = color(
        ui.ctx(),
        Color32::from_rgb(215, 201, 252),
        Color32::from_rgb(91, 80, 140),
    );
    let bg = color(
        ui.ctx(),
        Color32::from_rgb(57, 49, 81),
        Color32::from_rgb(237, 232, 249),
    );
    let galley =
        ui.fonts(|fonts| fonts.layout_no_wrap("插件".into(), egui::FontId::proportional(10.5), fg));
    let tag_size = galley.size() + egui::vec2(12.0, 2.0);
    ui.horizontal(|ui| {
        let title_width =
            (ui.available_width() - tag_size.x - ui.spacing().item_spacing.x).max(0.0);
        ui.scope(|ui| {
            ui.set_max_width(title_width);
            ui.add(egui::Label::new(title).truncate())
                .on_hover_text(title);
        });
        let (rect, _) = ui.allocate_exact_size(tag_size, egui::Sense::hover());
        ui.painter().rect_filled(rect, 4.0, bg);
        let pos = rect.min + egui::vec2(6.0, 1.0);
        ui.painter().galley(pos, galley.clone(), fg);
        ui.painter().galley(pos + egui::vec2(0.3, 0.0), galley, fg);
    });
}

fn plugin_switch(ui: &mut egui::Ui, title: &str, on: bool) -> bool {
    let (_, rect) = ui.allocate_space(egui::vec2(38.0, 22.0));
    let response = ui.interact(
        rect,
        ui.make_persistent_id("plugin-switch"),
        egui::Sense::click(),
    );
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Checkbox, response.enabled(), on, title)
    });
    let t = ui
        .ctx()
        .animate_bool_with_time(response.id.with("t"), on, 0.15);
    let accent = color(
        ui.ctx(),
        Color32::from_rgb(140, 127, 208),
        Color32::from_rgb(91, 80, 140),
    );
    let off = color(
        ui.ctx(),
        Color32::from_gray(86),
        Color32::from_rgb(203, 207, 214),
    );
    ui.painter()
        .rect_filled(rect, 11.0, paint::mix(off, accent, t));
    if response.has_focus() {
        ui.painter().rect_stroke(
            rect.expand(2.0),
            13.0,
            egui::Stroke::new(1.0, accent),
            egui::StrokeKind::Outside,
        );
    }
    let x = egui::lerp((rect.left() + 11.0)..=(rect.right() - 11.0), t);
    ui.painter()
        .circle_filled(egui::pos2(x, rect.center().y), 8.0, Color32::WHITE);
    response.on_hover_text(title).clicked()
}

fn authorize(
    host: &mut PluginQuotaHost,
    ctx: &egui::Context,
    id: &str,
    address: &mut String,
    error: &mut Option<String>,
) -> bool {
    let Some(record) = host.records().find(|record| record.id == id) else {
        return true;
    };
    let Some(kind) = record.source_kind else {
        return true;
    };
    let mut grant = false;
    let mut revoke = false;
    let modal = egui::Modal::new(egui::Id::new("local-quota-plugin-source"))
        .frame(settings_modal_frame(ctx))
        .backdrop_color(egui::Color32::from_black_alpha(110))
        .show(ctx, |ui| {
            ui.set_width(395.0);
            settings_apply_style(ui);
            let mut close = settings_modal_title(ui, "只读数据源");
            ui.add(egui::Label::new(record.name).truncate());
            if let Some(grant) = record.source_grant {
                settings_hint(ui, "已授权读取");
                let target = match grant {
                    SourceGrant::File(path) => path.to_string_lossy(),
                    SourceGrant::Loopback(url) => url.as_str().into(),
                };
                ui.add(egui::Label::new(target.as_ref()).truncate())
                    .on_hover_text(target.as_ref());
            }
            match kind {
                SourceKind::File => {
                    settings_hint(ui, "仅在插件启用时读取你选中的一个本地 UTF-8 文件。");
                    grant = ui
                        .add_enabled(cfg!(windows), egui::Button::new("选择并授权文件…"))
                        .clicked();
                }
                SourceKind::Loopback => {
                    settings_hint(ui, "仅在插件启用时向授权的本机接口地址发送 GET 请求。");
                    ui.add(
                        egui::TextEdit::singleline(address)
                            .char_limit(2048)
                            .desired_width(385.0)
                            .hint_text("http://127.0.0.1:8080/status"),
                    );
                    grant = ui
                        .add_enabled(
                            !address.trim().is_empty(),
                            egui::Button::new("授权读取此地址"),
                        )
                        .clicked();
                }
            }
            show_error(ui, error.as_deref());
            ui.horizontal(|ui| {
                if record.source_grant.is_some() {
                    revoke = ui.button("撤销授权").clicked();
                }
                close |= ui.button("完成").clicked();
            });
            close
        });
    if modal.inner || modal.should_close() {
        return true;
    }
    if grant || revoke {
        *error = if revoke {
            host.revoke(id).inspect(|_| address.clear()).err()
        } else {
            match kind {
                SourceKind::File => pick_file("选择要授权读取的本地文件")
                    .and_then(|path| path.map_or(Ok(()), |path| host.grant_file(id, &path)))
                    .err(),
                SourceKind::Loopback => host.grant_url(id, address).err(),
            }
        };
        ctx.request_repaint_of(egui::ViewportId::ROOT);
    }
    false
}

fn uninstall(
    host: &mut PluginQuotaHost,
    ctx: &egui::Context,
    id: &str,
    error: &mut Option<String>,
) -> bool {
    let Some(record) = host.records().find(|record| record.id == id) else {
        return true;
    };
    let mut confirm = false;
    let modal = egui::Modal::new(egui::Id::new("local-quota-plugin-uninstall"))
        .frame(settings_modal_frame(ctx))
        .backdrop_color(egui::Color32::from_black_alpha(110))
        .show(ctx, |ui| {
            ui.set_width(360.0);
            settings_apply_style(ui);
            let mut close = settings_modal_title(ui, "卸载额度插件");
            ui.add(egui::Label::new(record.name).truncate());
            settings_hint(ui, "卸载后将撤销授权，并删除此插件在岛仔中的配置与数据。");
            show_error(ui, error.as_deref());
            ui.horizontal(|ui| {
                close |= ui.button("取消").clicked();
                confirm = ui.button("卸载").clicked();
            });
            close
        });
    if confirm {
        *error = host.forget(id).err();
        ctx.request_repaint_of(egui::ViewportId::ROOT);
        return error.is_none();
    }
    modal.inner || modal.should_close()
}

fn show_error(ui: &mut egui::Ui, error: Option<&str>) {
    if let Some(error) = error {
        let color = color(
            ui.ctx(),
            egui::Color32::from_rgb(231, 175, 108),
            egui::Color32::from_rgb(151, 94, 28),
        );
        ui.add(egui::Label::new(egui::RichText::new(error).color(color)).wrap());
    }
}

fn pick_file(title: &str) -> Result<Option<PathBuf>, String> {
    #[cfg(windows)]
    {
        crate::system::pick_files_for(title, false)
            .map(|paths| paths.into_iter().next())
            .map_err(|error| format!("无法打开文件选择器（{}）", error.code()))
    }
    #[cfg(not(windows))]
    {
        let _ = title;
        Err("当前系统不支持文件选择".into())
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::{fs, time::SystemTime};

    #[test]
    fn plugin_quota_settings_show_install_and_follow_the_switch() {
        struct Scratch(PathBuf);
        impl Drop for Scratch {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let nonce = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "daozai-quota-settings-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&dir).unwrap();
        let scratch = Scratch(dir);
        let id = "test.quota-settings";
        let manifest = scratch.0.join("plugin.json");
        let manifest_json = serde_json::json!({
            "id": id, "name": "模拟额度", "author": "测试", "version": "1.0.0", "api": 1,
            "dataVersion": 1, "owner": "quota", "runtime": "js", "entry": "main.js",
            "cover": "cover.png", "capabilities": ["quota.source", "timer"],
            "permissions": {"source.read": "file"}, "settings": []
        });
        fs::write(&manifest, manifest_json.to_string()).unwrap();
        fs::write(
            scratch.0.join("main.js"),
            "function refresh() { return {quota:null}; }",
        )
        .unwrap();
        image::RgbaImage::new(1, 1)
            .save(scratch.0.join("cover.png"))
            .unwrap();
        let mut host = PluginQuotaHost::open(scratch.0.join("local-plugins"));
        host.install(&manifest).unwrap();

        let ctx = egui::Context::default();
        let draw = |host: &mut PluginQuotaHost, events| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(390.0, 280.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| settings(host, ui));
                },
            )
        };
        let shows = |frame: &egui::FullOutput, label| {
            frame.shapes.iter().any(|item| matches!(&item.shape, egui::Shape::Text(text) if text.galley.text() == label))
        };
        let frame = draw(&mut host, Vec::new());
        assert!(shows(&frame, "安装插件…"));
        assert!(shows(&frame, "模拟额度"));
        assert!(shows(&frame, "插件"));
        assert!(shows(&frame, "已停用"));
        let position = frame
            .shapes
            .iter()
            .find_map(|item| match &item.shape {
                egui::Shape::Rect(shape) if shape.rect.size() == egui::vec2(38.0, 22.0) => {
                    Some(shape.rect.center())
                }
                _ => None,
            })
            .expect("未绘制插件开关");
        let click_switch = |host: &mut PluginQuotaHost| {
            draw(host, vec![egui::Event::PointerMoved(position)]);
            for pressed in [true, false] {
                draw(
                    host,
                    vec![egui::Event::PointerButton {
                        pos: position,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    }],
                );
            }
        };
        click_switch(&mut host);
        assert!(host.records().next().unwrap().enabled);
        assert!(shows(&draw(&mut host, Vec::new()), "等待数据源授权"));
        click_switch(&mut host);
        assert!(!host.records().next().unwrap().enabled);
        assert!(shows(&draw(&mut host, Vec::new()), "已停用"));
    }
}
