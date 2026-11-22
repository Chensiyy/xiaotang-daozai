#[cfg(windows)]
use super::market_state::MarketStatus;
use super::*;
use crate::{
    extras::{settings_card, settings_control_row, settings_hint},
    paint::{self, Card, H},
    settings_theme::color,
    theme::{self, W},
};
use eframe::egui::{self, Color32};
#[cfg(windows)]
use std::sync::Arc;

#[cfg(windows)]
#[path = "market_browse_ui.rs"]
mod market_browse_ui;
#[path = "plugin_feedback_ui.rs"]
mod plugin_feedback_ui;
use plugin_feedback_ui::{plugin_feedback, request_plugin_feedback};

const COVERS: &str = "settings-plugin-covers";
type CoverCache = std::collections::VecDeque<(egui::Id, Option<egui::TextureHandle>)>;

fn forget_covers(ui: &egui::Ui) {
    ui.data_mut(|data| data.remove::<CoverCache>(egui::Id::new(COVERS)));
}

fn plugin_cover(
    ui: &mut egui::Ui,
    key: egui::Id,
    width: f32,
    radius: egui::CornerRadius,
    placeholder: &str,
    load: impl FnOnce() -> Option<std::result::Result<Vec<u8>, ()>>,
) -> bool {
    let size = egui::vec2(width, width * 9.0 / 16.0);
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let cache_id = egui::Id::new(COVERS);
    let cached = ui.data_mut(|data| {
        let cache = data.get_temp_mut_or_default::<CoverCache>(cache_id);
        let index = cache.iter().position(|(id, _)| *id == key)?;
        let entry = cache.remove(index)?;
        let texture = entry.1.clone();
        cache.push_front(entry);
        Some(texture)
    });
    if !ui.is_rect_visible(rect) {
        return cached.is_some_and(|texture| texture.is_some());
    }
    let frame = ui.ctx().cumulative_pass_nr();
    let queued = cached.is_none()
        && !ui.data_mut(|data| {
            let budget = data.get_temp_mut_or_default::<(u64, u8)>(cache_id.with("decode"));
            if budget.0 != frame {
                *budget = (frame, 0);
            }
            if budget.1 == 2 {
                return false;
            }
            budget.1 += 1;
            true
        });
    let mut pending = queued;
    let texture = if queued {
        ui.ctx().request_repaint();
        None
    } else {
        cached.unwrap_or_else(|| {
            let Some(result) = load() else {
                pending = true;
                return None;
            };
            let texture = result.ok().and_then(|png| {
                let image =
                    image::load_from_memory_with_format(&png, image::ImageFormat::Png).ok()?;
                let (w, h) = (image.width(), image.height());
                let crop_w = w.min((h * 16 / 9).max(1));
                let crop_h = h.min((w * 9 / 16).max(1));
                let pixels = image
                    .crop_imm((w - crop_w) / 2, (h - crop_h) / 2, crop_w, crop_h)
                    .thumbnail(768, 432)
                    .to_rgba8();
                let size = [pixels.width() as usize, pixels.height() as usize];
                let image = egui::ColorImage::from_rgba_unmultiplied(size, pixels.as_raw());
                Some(
                    ui.ctx()
                        .load_texture("plugin-cover", image, egui::TextureOptions::LINEAR),
                )
            });
            ui.data_mut(|data| {
                let cache = data.get_temp_mut_or_default::<CoverCache>(cache_id);
                cache.push_front((key, texture.clone()));
                cache.truncate(24);
            });
            texture
        })
    };
    ui.painter().rect_filled(
        rect,
        radius,
        color(
            ui.ctx(),
            Color32::from_rgb(45, 45, 50),
            Color32::from_rgb(233, 236, 241),
        ),
    );
    if let Some(texture) = texture {
        egui::Image::new(&texture)
            .corner_radius(radius)
            .paint_at(ui, rect);
        true
    } else {
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            if pending {
                "正在加载封面…"
            } else {
                placeholder
            },
            egui::FontId::proportional(11.5),
            color(ui.ctx(), Color32::from_gray(170), Color32::from_gray(92)),
        );
        false
    }
}

fn plugin_title(ui: &mut egui::Ui, title: egui::RichText, name: &str, author: &str, version: &str) {
    ui.add(egui::Label::new(title.size(15.0).strong()).truncate())
        .on_hover_text(name);
    let metadata = format!("{author} · v{version}");
    let muted = color(ui.ctx(), Color32::from_gray(175), Color32::from_gray(90));
    ui.add(egui::Label::new(egui::RichText::new(&metadata).size(11.5).color(muted)).truncate())
        .on_hover_text(metadata);
}

fn plugin_accent(ui: &egui::Ui) -> Color32 {
    color(
        ui.ctx(),
        Color32::from_rgb(110, 95, 166),
        Color32::from_rgb(91, 80, 140),
    )
}

fn plugin_switch(ui: &mut egui::Ui, id: &str, on: bool) -> bool {
    let (_, rect) = ui.allocate_space(egui::vec2(39.0, 22.0));
    let response = ui.interact(
        rect,
        egui::Id::new(("settings-plugin-switch", id)),
        egui::Sense::click(),
    );
    let label = if on { "停用插件" } else { "启用插件" };
    response
        .widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, on, label));
    let fill = if on {
        plugin_accent(ui)
    } else {
        color(
            ui.ctx(),
            Color32::from_gray(86),
            Color32::from_rgb(203, 207, 214),
        )
    };
    ui.painter().rect_filled(rect, 11.0, fill);
    if response.has_focus() {
        ui.painter().rect_stroke(
            rect.expand(2.0),
            13.0,
            egui::Stroke::new(1.0, plugin_accent(ui)),
            egui::StrokeKind::Outside,
        );
    }
    let x = if on {
        rect.right() - 11.0
    } else {
        rect.left() + 11.0
    };
    ui.painter()
        .circle_filled(egui::pos2(x, rect.center().y), 7.0, Color32::WHITE);
    response.on_hover_text(label).clicked()
}

#[cfg(windows)]
fn market_button(ui: &egui::Ui, text: &str) -> egui::Button<'static> {
    egui::Button::new(egui::RichText::new(text).color(Color32::WHITE))
        .fill(plugin_accent(ui))
        .corner_radius(6.0)
}

#[cfg(windows)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum PluginPage {
    Market,
    Mine,
    Favorites,
    TestMarket,
}

#[cfg(windows)]
fn open_plugin_details(ui: &egui::Ui, id: &str) {
    ui.data_mut(|data| data.insert_temp(egui::Id::new("plugin-market-open-detail"), id.to_owned()));
    ui.ctx().request_repaint();
}

#[cfg(windows)]
fn plugin_tabs(ui: &mut egui::Ui, page: &mut PluginPage, installed: usize) {
    let accent = color(
        ui.ctx(),
        Color32::from_rgb(165, 148, 228),
        Color32::from_rgb(116, 99, 182),
    );
    let muted = color(
        ui.ctx(),
        Color32::from_gray(180),
        Color32::from_rgb(88, 97, 108),
    );
    let line = color(
        ui.ctx(),
        Color32::from_rgb(55, 55, 62),
        Color32::from_rgb(214, 218, 226),
    );
    let left = ui.cursor().left();
    let right = left + ui.available_width();
    let mut active_rect = egui::Rect::NOTHING;
    let row = ui
        .horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 20.0;
            ui.spacing_mut().button_padding = egui::vec2(1.0, 4.0);
            let mine = format!("我的插件 · {installed}");
            for (target, title) in [
                (PluginPage::Market, "插件集市"),
                (PluginPage::Favorites, "我的收藏"),
                (PluginPage::Mine, mine.as_str()),
            ] {
                let selected = target == *page
                    || (target == PluginPage::Mine && *page == PluginPage::TestMarket);
                let text = egui::RichText::new(title).size(13.0);
                let text = if selected {
                    text.strong()
                } else {
                    text.color(muted)
                };
                let response = ui.add(egui::Button::new(text).frame(false));
                if response.has_focus() {
                    ui.painter().rect_stroke(
                        response.rect,
                        4.0,
                        egui::Stroke::new(1.0, accent),
                        egui::StrokeKind::Inside,
                    );
                }
                if response.clicked() {
                    *page = target;
                    active_rect = response.rect;
                } else if selected {
                    active_rect = response.rect;
                }
            }
        })
        .response
        .rect;
    ui.painter()
        .hline(left..=right, row.bottom(), egui::Stroke::new(1.0, line));
    let underline = egui::Rect::from_min_max(
        egui::pos2(active_rect.left(), row.bottom() - 2.0),
        egui::pos2(active_rect.right(), row.bottom() + 1.0),
    );
    ui.painter().rect_filled(underline, 1.0, accent);
    ui.add_space(13.0);
}

#[cfg(all(windows, feature = "sdk-material"))]
const CAPTURE_PERMISSION: &str = "读取岛体背后一小块屏幕画面，只在本机用于绘制材质，不保存、不上传；插件本身没有网络和文件权限。";

#[cfg(all(windows, feature = "sdk-material"))]
fn material_name(ui: &egui::Ui, name: &str, selected: bool, active: bool) -> egui::RichText {
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
    pub fn card(&mut self, c: &Card) -> bool {
        let mut cards = self.cards.iter().filter(|(id, _)| {
            self.records[*id].package.as_ref().unwrap().manifest.owner == "tasks"
        });
        let count = cards.clone().count();
        if count == 0 {
            return false;
        }
        self.card_index %= count;
        let (id, body) = cards.nth(self.card_index).unwrap();
        c.fill(42., 130., 335., 40., 9., theme::CARD);
        let manifest = &self.records[id].package.as_ref().unwrap().manifest;
        let has_settings = !manifest.settings.is_empty();
        c.text(
            52.,
            145.,
            &c.fit(&manifest.name, 11., W::Regular, 220.),
            11.,
            W::Regular,
            theme::CYAN,
            H::Left,
        );
        c.text(
            52.,
            163.,
            &c.fit(
                body,
                14.,
                W::Semibold,
                if has_settings { 290. } else { 305. },
            ),
            14.,
            W::Semibold,
            theme::WHITE,
            H::Left,
        );
        if count > 1 {
            c.text(
                if has_settings { 343. } else { 367. },
                145.,
                &format!("{}/{} ›", self.card_index + 1, count),
                11.,
                W::Regular,
                theme::GRAY,
                H::Right,
            );
            if c.hit(
                42.,
                130.,
                if has_settings { 307. } else { 335. },
                40.,
                "local-plugin-next",
            )
            .on_hover_text("下一张卡片")
            .clicked()
            {
                self.card_index = (self.card_index + 1) % count;
            }
        }
        if has_settings {
            let response = c
                .hit(349., 130., 28., 40., "local-plugin-settings")
                .on_hover_text("打开插件设置");
            let (hover, press) = c.hover_t(&response);
            c.circle(
                363.,
                145.,
                11.,
                egui::Color32::from_white_alpha((hover * 24.) as u8),
            );
            paint::gear(
                &c.scaled(363., 145., 0.8 * (1. - press * 0.08)),
                363.,
                145.,
                paint::mix(theme::GRAY, theme::WHITE, hover),
            );
            if response.clicked() {
                self.focus = Some(id.clone());
                return true;
            }
        }
        false
    }

    pub fn manage(
        &mut self,
        ui: &mut egui::Ui,
        tasks_on: bool,
        calendar_on: bool,
        #[cfg(all(windows, feature = "sdk-material"))] active_material: Option<&str>,
    ) -> Option<usize> {
        let mut jump = None;
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
        #[cfg(windows)]
        let (page_id, page) = {
            let was_busy = self.market_busy();
            self.poll_market();
            if self.market_pending() {
                ui.ctx().request_repaint_after(Duration::from_millis(32));
            }
            if was_busy && !self.market_busy() {
                ui.ctx().request_repaint_of(egui::ViewportId::ROOT);
            }
            let id = egui::Id::new("settings-plugins-market");
            let previous =
                ui.data(|data| data.get_temp::<PluginPage>(id))
                    .unwrap_or(if ids.is_empty() {
                        PluginPage::Market
                    } else {
                        PluginPage::Mine
                    });
            let detail = ui.data_mut(|data| {
                data.remove_temp::<String>(egui::Id::new("plugin-market-open-detail"))
            });
            let mut page = if detail.is_some() { PluginPage::Market } else { previous };
            plugin_tabs(ui, &mut page, ids.len());
            if page != previous {
                ui.data_mut(|data| data.insert_temp(id, page));
                market_browse_ui::close_detail(ui);
            }
            if let Some(detail) = detail.filter(|_| page == PluginPage::Market) {
                market_browse_ui::select_detail(ui, &detail);
            }
            if page == PluginPage::Mine && self.records.values().any(|record| record.official_market) {
                self.ensure_official_catalog();
            }
            (id, page)
        };
        if !self.notice.is_empty() {
            settings_hint(ui, &self.notice);
            ui.add_space(4.0);
        }
        #[cfg(windows)]
        if page != PluginPage::Mine {
            if self.market(ui, page != PluginPage::TestMarket, page == PluginPage::Favorites) {
                market_browse_ui::close_detail(ui);
                ui.data_mut(|data| data.insert_temp(page_id, PluginPage::Mine));
            }
            if self.market_pending() {
                ui.ctx().request_repaint_after(Duration::from_millis(32));
            }
            plugin_feedback(ui);
            return None;
        }
        for id in &self.invalid {
            ui.label(format!("{id} · 需要重新安装"));
        }
        if ids.is_empty() && self.invalid.is_empty() {
            settings_card(ui, |ui| {
                ui.strong("还没有安装插件");
                settings_hint(ui, "装好后可以在这里管理。");
                #[cfg(windows)]
                if ui.add(market_button(ui, "去插件集市")).clicked() {
                    ui.data_mut(|data| data.insert_temp(page_id, PluginPage::Market));
                }
            });
        }
        for id in ids {
            ui.push_id(&id, |ui| {
                let fill = color(ui.ctx(), Color32::from_rgb(35, 35, 38), Color32::WHITE);
                let border = color(
                    ui.ctx(),
                    Color32::from_rgb(58, 58, 63),
                    Color32::from_rgb(224, 226, 232),
                );
                egui::Frame::new()
                    .fill(fill)
                    .stroke(egui::Stroke::new(1.0, border))
                    .corner_radius(10.0)
                    .inner_margin(egui::Margin::symmetric(12, 8))
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        let r = &self.records[&id];
                        let package = r.package.as_ref().unwrap();
                        let manifest = &package.manifest;
                        let material = manifest.is_material();
                        let calendar = manifest.owner == "calendar";
                        let retired = manifest.is_retired_lunar_preview();
                        let owner = if calendar { calendar_on } else { tasks_on };
                        let on = r.enabled;
                        let has_settings = !manifest.settings.is_empty();
                        let key = egui::Id::new(("installed", &r.origin, &id, &manifest.version));
                        let cover = package.cover.clone();
                        let runtime_error = self.errors.get(&id).cloned();
                        let (name, author, version) = (
                            manifest.name.clone(),
                            manifest.author.clone(),
                            manifest.version.clone(),
                        );
                        #[cfg(windows)]
                        let (listed, official_mark, preview_sample) = {
                            let catalog = self.market_catalog(true);
                            let entry = catalog.as_ref().and_then(|catalog| {
                                catalog.entries.iter().find(|entry| entry.id == id)
                            });
                            let presentation = match entry {
                                Some(entry) => entry.presentation.as_ref(),
                                None => r.presentation.as_ref(),
                            };
                            let trusted = presentation.filter(|_| r.official_market);
                            (
                                entry.is_some(),
                                trusted.is_some_and(|meta| meta.official),
                                trusted.is_some_and(|meta| meta.sample),
                            )
                        };
                        let title = egui::RichText::new(&name);
                        #[cfg(all(windows, feature = "sdk-material"))]
                        let selected = self.selected_material_id() == Some(id.as_str());
                        #[cfg(all(windows, feature = "sdk-material"))]
                        let title = if material {
                            material_name(ui, &name, selected, active_material == Some(id.as_str()))
                        } else {
                            title
                        };
                        let identity = |ui: &mut egui::Ui| {
                            ui.spacing_mut().item_spacing.x = 10.0;
                            plugin_cover(
                                ui,
                                key,
                                72.0,
                                egui::CornerRadius::same(7),
                                "暂无封面",
                                || {
                                    Some(
                                        cover
                                            .as_ref()
                                            .map(|cover| cover.bytes().to_vec())
                                            .ok_or(()),
                                    )
                                },
                            );
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 3.0;
                                #[cfg(windows)]
                                let author =
                                    market_browse_ui::display_author(&author, official_mark);
                                #[cfg(not(windows))]
                                let author = author.as_str();
                                plugin_title(ui, title, &name, author, &version);
                                if material {
                                    #[cfg(all(windows, feature = "sdk-material"))]
                                    settings_hint(
                                        ui,
                                        if selected {
                                            "外观材质 · 已选用"
                                        } else {
                                            "外观材质 · 已停用"
                                        },
                                    );
                                } else {
                                    let (state, tint) = if retired {
                                        (
                                            "已撤回 · 等待更新",
                                            color(
                                                ui.ctx(),
                                                Color32::from_gray(175),
                                                Color32::from_gray(90),
                                            ),
                                        )
                                    } else if !on {
                                        (
                                            "已停用",
                                            color(
                                                ui.ctx(),
                                                Color32::from_gray(175),
                                                Color32::from_gray(90),
                                            ),
                                        )
                                    } else if !owner {
                                        (
                                            "已开启 · 模块已暂停",
                                            color(
                                                ui.ctx(),
                                                Color32::from_rgb(238, 181, 91),
                                                Color32::from_rgb(139, 88, 12),
                                            ),
                                        )
                                    } else {
                                        (
                                            "已启用",
                                            color(
                                                ui.ctx(),
                                                Color32::from_rgb(127, 214, 155),
                                                Color32::from_rgb(29, 122, 60),
                                            ),
                                        )
                                    };
                                    ui.horizontal_wrapped(|ui| {
                                        ui.spacing_mut().item_spacing.x = 5.0;
                                        let (_, rect) = ui.allocate_space(egui::vec2(7.0, 13.0));
                                        ui.painter().circle_filled(rect.center(), 3.5, tint);
                                        ui.label(egui::RichText::new(state).size(11.5).color(tint));
                                        #[cfg(windows)]
                                        market_browse_ui::show_badge(
                                            ui,
                                            official_mark,
                                            preview_sample,
                                        );
                                    });
                                    if let Some(e) = runtime_error {
                                        settings_hint(ui, &format!("刷新失败 · {e}"));
                                    }
                                }
                            });
                        };
                        let actions = |ui: &mut egui::Ui| {
                            ui.spacing_mut().item_spacing.x = 8.0;
                            #[cfg(windows)]
                            ui.menu_button("更多", |ui| {
                                if listed && ui.button("查看详情").clicked() {
                                    open_plugin_details(ui, &id);
                                    ui.close_menu();
                                }
                                if ui.button("反馈问题").clicked() {
                                    request_plugin_feedback(ui, &id, &name, &version);
                                    ui.close_menu();
                                }
                            });
                            if ui.add(egui::Button::new("卸载").frame(false)).clicked() {
                                self.pending = Some(id.clone());
                            }
                            if material {
                                #[cfg(all(windows, feature = "sdk-material"))]
                                {
                                    if ui.button(if selected { "停用" } else { "启用" }).clicked()
                                        && self.request_material(
                                            ui,
                                            (!selected).then_some(id.as_str()),
                                        )
                                        && !selected
                                    {
                                        jump = Some(crate::extras::SETTINGS_TAB_APPEARANCE);
                                    }
                                    if ui.button("前往设置").clicked() {
                                        jump = Some(crate::extras::SETTINGS_TAB_APPEARANCE);
                                    }
                                }
                            } else {
                                if plugin_switch(ui, &id, on) {
                                    self.notice = self
                                        .enable(&id, !on)
                                        .err()
                                        .map(|e| e.to_string())
                                        .unwrap_or_default();
                                    if self.notice.is_empty() {
                                        ui.ctx().request_repaint_of(egui::ViewportId::ROOT);
                                    }
                                }
                                if !retired
                                    && on
                                    && owner
                                    && has_settings
                                    && ui.button("前往设置").clicked()
                                {
                                    self.focus = Some(id.clone());
                                    let page = if calendar {
                                        crate::app::CALENDAR_PAGE
                                    } else {
                                        crate::app::TODO_PAGE
                                    };
                                    jump = Some(crate::app::PAGE_SETTINGS_TABS[page]);
                                }
                                if !retired && on && !owner && ui.button("前往页面管理").clicked()
                                {
                                    jump = Some(crate::extras::SETTINGS_TAB_PAGES);
                                }
                            }
                        };
                        let width = ui.available_width();
                        let aligned_actions = egui::Layout::right_to_left(egui::Align::Center);
                        if width >= 418.0 {
                            let information = width - 208.0;
                            let row_layout = egui::Layout::left_to_right(egui::Align::Center);
                            ui.allocate_ui_with_layout(egui::vec2(width, 52.0), row_layout, |ui| {
                                ui.allocate_ui_with_layout(
                                    egui::vec2(information, 52.0),
                                    row_layout,
                                    |ui| {
                                        ui.set_width(information);
                                        identity(ui);
                                    },
                                );
                                ui.with_layout(aligned_actions, actions);
                            });
                        } else {
                            ui.horizontal(identity);
                            ui.add_space(3.0);
                            ui.with_layout(aligned_actions.with_main_wrap(true), actions);
                        }
                        if self.pending.as_deref() == Some(&id) {
                            ui.label("卸载时如何处理设置和插件数据？");
                            ui.horizontal_wrapped(|ui| {
                                for (label, keep) in [("保留数据", true), ("删除数据", false)]
                                {
                                    if ui.button(label).clicked() {
                                        self.notice = self
                                            .uninstall(&id, keep)
                                            .err()
                                            .map(|e| e.to_string())
                                            .unwrap_or_default();
                                        if self.notice.is_empty() {
                                            forget_covers(ui);
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
        #[cfg(windows)]
        {
            ui.add_space(6.0);
            ui.collapsing("开发者模式", |ui| {
                if ui.button("导入测试集市目录…").clicked() {
                    match crate::system::pick_files_for("选择测试集市的 catalog.json", false)
                    {
                        Ok(paths) => {
                            if let Some(path) = paths.first() {
                                match self.set_market_catalog(path) {
                                    Ok(()) => {
                                        forget_covers(ui);
                                        self.notice.clear();
                                        ui.data_mut(|data| {
                                            data.insert_temp(page_id, PluginPage::TestMarket)
                                        });
                                    }
                                    Err(e) => self.notice = e.to_string(),
                                }
                            }
                        }
                        Err(e) => self.notice = e.to_string(),
                    }
                }
                if self.market_catalog(false).is_some() && ui.button("打开测试集市").clicked()
                {
                    self.notice.clear();
                    ui.data_mut(|data| data.insert_temp(page_id, PluginPage::TestMarket));
                }
                if ui.button("安装本地插件…").clicked() {
                    match crate::system::pick_files_for("选择本地插件的 plugin.json", false)
                    {
                        Ok(paths) => {
                            if let Some(path) = paths.first() {
                                self.notice = match self.install(path) {
                                    Ok(()) => {
                                        forget_covers(ui);
                                        "已安装。启用后开始运行。".into()
                                    }
                                    Err(e) => e.to_string(),
                                };
                            }
                        }
                        Err(e) => self.notice = e.to_string(),
                    }
                }
            });
        }
        #[cfg(all(windows, feature = "sdk-material"))]
        {
            if self.confirm_material_permission(ui) {
                jump = Some(crate::extras::SETTINGS_TAB_APPEARANCE);
            }
            material_error(ui);
        }
        plugin_feedback(ui);
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

    pub fn settings(&mut self, ui: &mut egui::Ui, owner: &str, module_on: bool) {
        if !module_on {
            return;
        }
        let ids: Vec<_> = self
            .records
            .iter()
            .filter(|(_, r)| {
                r.enabled
                    && r.package.as_ref().is_some_and(|p| {
                        p.manifest.owner == owner && !p.manifest.settings.is_empty()
                    })
            })
            .map(|(id, _)| id.clone())
            .collect();
        if ids.is_empty() {
            return;
        }
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
                            Ok(()) => {
                                ui.ctx().request_repaint_of(egui::ViewportId::ROOT);
                                "设置已保存".into()
                            }
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
