use super::*;
use crate::{
    extras::{settings_card, settings_control_row},
    paint::{Card, H},
    theme::{self, W},
};
use eframe::egui;

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
    pub fn manage(&mut self, ui: &mut egui::Ui, owner: bool) -> bool {
        let mut jump = false;
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
            .filter(|(_, r)| r.package.is_some())
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
                    let on = r.enabled;
                    ui.strong(&r.package.as_ref().unwrap().manifest.name);
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
                            jump = true;
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
        jump
    }
    pub fn settings(&mut self, ui: &mut egui::Ui) {
        let ids: Vec<_> = self
            .records
            .iter()
            .filter(|(_, r)| r.package.is_some())
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
