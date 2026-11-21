use super::*;
use crate::{
    extras::{settings_card, settings_control_row},
    paint::{Card, H},
    theme::{self, W},
};
use eframe::egui;

#[cfg(all(windows, feature = "sdk-material"))]
const CAPTURE_PERMISSION: &str = "读取岛体背后一小块屏幕画面，只在本机用于绘制材质，不保存、不上传；插件本身没有网络和文件权限。";

#[cfg(all(windows, feature = "sdk-material"))]
fn material_name(ui: &egui::Ui, name: &str, selected: bool, active: bool) -> egui::RichText {
    use crate::settings_theme::color;
    use egui::Color32;
    let title = egui::RichText::new(name);
    if !selected {
        return title;
    }
    let ctx = ui.ctx();
    let color = if active {
        color(
            ctx,
            Color32::from_rgb(127, 214, 155),
            Color32::from_rgb(29, 122, 60),
        )
    } else if crate::bar_layout::animations_enabled() {
        let time = ui.input(|input| input.time);
        let phase = (0.5 + 0.5 * (time * std::f64::consts::TAU * 0.8).sin()) as f32;
        ctx.request_repaint_after(Duration::from_millis(80));
        crate::paint::mix(
            color(
                ctx,
                Color32::from_rgb(216, 162, 86),
                Color32::from_rgb(139, 88, 12),
            ),
            color(
                ctx,
                Color32::from_rgb(250, 204, 135),
                Color32::from_rgb(165, 111, 27),
            ),
            phase,
        )
    } else {
        color(
            ctx,
            Color32::from_rgb(238, 181, 91),
            Color32::from_rgb(139, 88, 12),
        )
    };
    title.color(color)
}

#[cfg(all(windows, feature = "sdk-material"))]
fn material_changed(ui: &egui::Ui, result: Result<()>) -> bool {
    let id = egui::Id::new("settings-material-error");
    match result {
        Ok(()) => {
            ui.data_mut(|data| data.remove::<String>(id));
            ui.ctx().request_repaint_of(egui::ViewportId::ROOT);
            true
        }
        Err(error) => {
            ui.data_mut(|data| data.insert_temp(id, error.to_string()));
            false
        }
    }
}

#[cfg(all(windows, feature = "sdk-material"))]
fn material_error(ui: &mut egui::Ui) {
    if let Some(error) =
        ui.data(|data| data.get_temp::<String>(egui::Id::new("settings-material-error")))
    {
        crate::extras::settings_hint(ui, &error);
    }
}

impl LocalPlugins {
    pub fn card(&mut self, c: &Card) {
        let count = self.cards.len();
        if count == 0 {
            return;
        }
        self.card_index %= count;
        let (id, body) = self.cards.iter().nth(self.card_index).unwrap();
        c.fill(42., 130., 335., 40., 9., theme::CARD);
        let name = &self.records[id].package.as_ref().unwrap().manifest.name;
        c.text(
            52.,
            145.,
            &c.fit(name, 11., W::Regular, 220.),
            11.,
            W::Regular,
            theme::CYAN,
            H::Left,
        );
        c.text(
            52.,
            163.,
            &c.fit(body, 14., W::Semibold, 305.),
            14.,
            W::Semibold,
            theme::WHITE,
            H::Left,
        );
        if count > 1 {
            c.text(
                367.,
                145.,
                &format!("{}/{} ›", self.card_index + 1, count),
                11.,
                W::Regular,
                theme::GRAY,
                H::Right,
            );
            if c.hit(42., 130., 335., 40., "local-plugin-next")
                .on_hover_text("下一张卡片")
                .clicked()
            {
                self.card_index = (self.card_index + 1) % count;
            }
        }
    }
    pub fn manage(
        &mut self,
        ui: &mut egui::Ui,
        owner: bool,
        #[cfg(all(windows, feature = "sdk-material"))] active_material: Option<&str>,
    ) -> Option<usize> {
        let mut jump = None;
        ui.collapsing("开发者模式", |ui| {
            #[cfg(windows)]
            if ui.button("安装本地插件…").clicked() {
                match crate::system::pick_files_for("选择本地插件的 plugin.json", false) {
                    Ok(paths) => {
                        if let Some(path) = paths.first() {
                            self.notice = match self.install(path) {
                                Ok(()) => "已安装。启用后开始运行。".into(),
                                Err(e) => e.to_string(),
                            };
                        }
                    }
                    Err(e) => self.notice = e.to_string(),
                }
            }
        });
        if !self.notice.is_empty() {
            ui.label(&self.notice);
        }
        let ids: Vec<_> = self
            .records
            .iter()
            .filter(|(_, r)| {
                r.package.as_ref().is_some_and(|p| {
                    cfg!(all(windows, feature = "sdk-material")) || !p.manifest.is_material()
                })
            })
            .map(|(id, _)| id.clone())
            .collect();
        for id in &self.invalid {
            ui.label(format!("{id} · 需要重新安装"));
        }
        if ids.is_empty() && self.invalid.is_empty() {
            ui.label("尚未安装插件");
        }
        for id in ids {
            ui.push_id(&id, |ui| {
                settings_card(ui, |ui| {
                    let r = &self.records[&id];
                    let manifest = &r.package.as_ref().unwrap().manifest;
                    if manifest.is_material() {
                        #[cfg(all(windows, feature = "sdk-material"))]
                        {
                            let selected = self.selected_material_id() == Some(id.as_str());
                            ui.label(
                                material_name(
                                    ui,
                                    &manifest.name,
                                    selected,
                                    active_material == Some(id.as_str()),
                                )
                                .strong(),
                            );
                            ui.horizontal_wrapped(|ui| {
                                if ui.button("前往设置").clicked() {
                                    jump = Some(crate::extras::SETTINGS_TAB_APPEARANCE);
                                }
                                if ui.button(if selected { "停用" } else { "启用" }).clicked()
                                    && self.request_material(ui, (!selected).then_some(id.as_str()))
                                    && !selected
                                {
                                    jump = Some(crate::extras::SETTINGS_TAB_APPEARANCE);
                                }
                                if ui.button("卸载").clicked() {
                                    self.pending = Some(id.clone());
                                }
                            });
                        }
                    } else {
                        let on = r.enabled;
                        ui.strong(&manifest.name);
                        ui.label(if !on {
                            "已停用"
                        } else if !owner {
                            "所属待办模块已停用"
                        } else {
                            "已启用"
                        });
                        if let Some(e) = self.errors.get(&id) {
                            ui.label(format!("刷新失败 · {e}"));
                        }
                        ui.horizontal_wrapped(|ui| {
                            if ui.button("前往设置").clicked() {
                                self.focus = Some(id.clone());
                                jump = Some(4);
                            }
                            if ui.button(if on { "停用" } else { "启用" }).clicked() {
                                self.notice = self
                                    .enable(&id, !on)
                                    .err()
                                    .map(|e| e.to_string())
                                    .unwrap_or_default();
                            }
                            if ui.button("卸载").clicked() {
                                self.pending = Some(id.clone());
                            }
                        });
                    }
                    if self.pending.as_deref() == Some(&id) {
                        ui.label("卸载时如何处理设置和插件数据？");
                        ui.horizontal_wrapped(|ui| {
                            for (label, keep) in [("保留数据", true), ("删除数据", false)] {
                                if ui.button(label).clicked() {
                                    self.notice = self
                                        .uninstall(&id, keep)
                                        .err()
                                        .map(|e| e.to_string())
                                        .unwrap_or_default();
                                    #[cfg(all(windows, feature = "sdk-material"))]
                                    if self.notice.is_empty() {
                                        ui.ctx().request_repaint_of(egui::ViewportId::ROOT);
                                    }
                                    self.pending = None;
                                }
                            }
                            if ui.button("取消").clicked() {
                                self.pending = None;
                            }
                        });
                    }
                })
            });
        }
        #[cfg(all(windows, feature = "sdk-material"))]
        {
            if self.confirm_material_permission(ui) {
                jump = Some(crate::extras::SETTINGS_TAB_APPEARANCE);
            }
            material_error(ui);
        }
        jump
    }

    #[cfg(all(windows, feature = "sdk-material"))]
    fn request_material(&mut self, ui: &egui::Ui, id: Option<&str>) -> bool {
        if let Some(id) = id.filter(|id| !self.material_permission_granted(id)) {
            ui.data_mut(|data| {
                data.insert_temp(egui::Id::new("settings-material-permission"), id.to_owned())
            });
            return false;
        }
        material_changed(ui, self.activate_material(id, false))
    }

    #[cfg(all(windows, feature = "sdk-material"))]
    fn confirm_material_permission(&mut self, ui: &egui::Ui) -> bool {
        use crate::extras::{
            settings_apply_style, settings_hint, settings_modal_frame, settings_modal_title,
        };
        let key = egui::Id::new("settings-material-permission");
        let Some(id) = ui.data(|data| data.get_temp::<String>(key)) else {
            return false;
        };
        let Some(package) = self.records.get(&id).and_then(Record::material_package) else {
            ui.data_mut(|data| data.remove::<String>(key));
            return false;
        };
        let ctx = ui.ctx();
        let mut close = false;
        let mut allow = false;
        let modal = egui::Modal::new(key.with("modal"))
            .frame(settings_modal_frame(ctx))
            .backdrop_color(egui::Color32::from_black_alpha(110))
            .show(ctx, |ui| {
                ui.set_width(360.0);
                settings_apply_style(ui);
                close = settings_modal_title(ui, "允许材质读取屏幕画面？");
                ui.add(
                    egui::Label::new(egui::RichText::new(&package.manifest.name).strong()).wrap(),
                );
                settings_hint(ui, CAPTURE_PERMISSION);
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    close |= ui.button("取消").clicked();
                    allow = ui.button("允许并启用").clicked();
                });
            });
        if allow || close || modal.should_close() {
            ui.data_mut(|data| data.remove::<String>(key));
        }
        allow && material_changed(ui, self.activate_material(Some(&id), true))
    }

    #[cfg(all(windows, feature = "sdk-material"))]
    pub(crate) fn material_settings(
        &mut self,
        ui: &mut egui::Ui,
        active: Option<&str>,
        compatibility_enabled: bool,
    ) -> bool {
        let selected = self.selected_material_id().map(str::to_owned);
        let mut choice = selected.clone();
        let title = selected.as_deref().map_or_else(
            || egui::RichText::new("无"),
            |id| {
                material_name(
                    ui,
                    &self.records[id].package.as_ref().unwrap().manifest.name,
                    true,
                    active == Some(id),
                )
            },
        );
        settings_control_row(ui, "材质", 68.0, |ui| {
            egui::ComboBox::from_id_salt("appearance-material")
                .selected_text(title)
                .width(ui.available_width().min(220.0))
                .truncate()
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut choice, None, "无");
                    for (id, record) in &self.records {
                        if let Some(package) = record.material_package() {
                            ui.selectable_value(
                                &mut choice,
                                Some(id.clone()),
                                &package.manifest.name,
                            );
                        }
                    }
                })
                .response
        });
        if choice != selected {
            self.request_material(ui, choice.as_deref());
        }
        self.confirm_material_permission(ui);
        material_error(ui);

        let Some(id) = self.selected_material_id().map(str::to_owned) else {
            return false;
        };
        let mut restart = false;
        if !compatibility_enabled && active != Some(id.as_str()) {
            crate::extras::settings_hint(
                ui,
                "材质需要兼容渲染（Direct3D 12），开启后重启岛仔生效。",
            );
            restart = ui.button("开启并重启").clicked();
        }
        let record = &self.records[&id];
        let fields = &record.material_package().unwrap().manifest.settings;
        let mut changes = Vec::new();
        for field in fields {
            let (Some(min), Some(max), Some(step)) = (field.min, field.max, field.step) else {
                continue;
            };
            let precision = field.precision.map_or_else(
                || {
                    step.to_string()
                        .split_once('.')
                        .map_or(0, |(_, fraction)| fraction.len())
                },
                usize::from,
            );
            let mut value = record
                .values
                .get(&field.key)
                .unwrap_or(&field.default)
                .parse::<f32>()
                .unwrap_or(min);
            let original = f64::from(value);
            let changed = ui
                .push_id((&id, &field.key), |ui| {
                    ui.horizontal(|ui| {
                        let label_width = (ui.available_width() - 180.0).max(56.0);
                        ui.add_sized(
                            [label_width, 24.0],
                            egui::Label::new(&field.label).truncate(),
                        )
                        .on_hover_text(&field.label);
                        ui.spacing_mut().slider_width =
                            (ui.available_width() - 65.0).clamp(54.0, 135.0);
                        ui.add(
                            egui::Slider::new(&mut value, min..=max)
                                .clamping(egui::SliderClamping::Edits)
                                .step_by(f64::from(step))
                                .suffix(field.unit.as_deref().unwrap_or_default())
                                .custom_formatter(move |number, _| format!("{number:.precision$}"))
                                .custom_parser(move |input| {
                                    if input == format!("{original:.precision$}") {
                                        Some(original)
                                    } else {
                                        input
                                            .chars()
                                            .filter(|c| !c.is_whitespace())
                                            .map(|c| if c == '−' { '-' } else { c })
                                            .collect::<String>()
                                            .parse()
                                            .ok()
                                    }
                                }),
                        )
                        .changed()
                    })
                    .inner
                })
                .inner;
            if changed {
                changes.push((field.key.clone(), value.clamp(min, max)));
            }
        }
        for (key, value) in changes {
            material_changed(ui, self.set_material_parameter(&id, &key, value));
        }
        restart
    }

    pub fn settings(&mut self, ui: &mut egui::Ui) {
        let ids: Vec<_> = self
            .records
            .iter()
            .filter(|(_, r)| {
                r.package
                    .as_ref()
                    .is_some_and(|p| !p.manifest.is_material())
            })
            .map(|(id, _)| id.clone())
            .collect();
        for id in ids {
            ui.push_id(&id, |ui| {
                settings_card(ui, |ui| {
                    let r = &self.records[&id];
                    let m = &r.package.as_ref().unwrap().manifest;
                    let header = ui.strong(&m.name);
                    if self.focus.as_deref() == Some(&id) {
                        header.scroll_to_me(Some(egui::Align::Center));
                        self.focus = None;
                    }
                    let fields = self
                        .drafts
                        .entry(id.clone())
                        .or_insert_with(|| r.values.clone());
                    for f in &m.settings {
                        settings_control_row(ui, &f.label, 68., |ui| {
                            ui.text_edit_singleline(
                                fields
                                    .entry(f.key.clone())
                                    .or_insert_with(|| f.default.clone()),
                            )
                        });
                    }
                    if ui.button("保存设置").clicked() {
                        let values = fields.clone();
                        self.notice = match self.save(&id, values) {
                            Ok(()) => "设置已保存".into(),
                            Err(e) => e.to_string(),
                        };
                    }
                })
            });
        }
        if !self.notice.is_empty() {
            ui.label(&self.notice);
        }
    }
}
