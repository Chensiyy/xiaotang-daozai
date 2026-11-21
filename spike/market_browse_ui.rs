use super::*;

fn detail_id() -> egui::Id {
    egui::Id::new("plugin-market-details")
}

pub(super) fn select_detail(ui: &egui::Ui, id: &str) {
    ui.data_mut(|data| data.insert_temp(detail_id(), id.to_owned()));
    ui.ctx().request_repaint();
}

pub(super) fn close_detail(ui: &egui::Ui) {
    ui.data_mut(|data| data.remove::<String>(detail_id()));
}

fn market_frame(ui: &egui::Ui) -> egui::Frame {
    egui::Frame::new()
        .fill(color(
            ui.ctx(),
            Color32::from_rgb(35, 35, 38),
            Color32::WHITE,
        ))
        .stroke(egui::Stroke::new(
            1.0,
            color(
                ui.ctx(),
                Color32::from_rgb(52, 52, 58),
                Color32::from_rgb(218, 221, 229),
            ),
        ))
        .corner_radius(10.0)
}

pub(super) fn show_badge(ui: &mut egui::Ui, official: bool, sample: bool) {
    let label = match (official, sample) {
        (true, _) => "官方",
        (false, true) => "验收样例",
        _ => return,
    };
    egui::Frame::new()
        .fill(color(
            ui.ctx(),
            Color32::from_rgb(57, 49, 81),
            Color32::from_rgb(237, 232, 249),
        ))
        .inner_margin(egui::Margin::symmetric(6, 2))
        .corner_radius(4.0)
        .show(ui, |ui| {
            ui.label(egui::RichText::new(label).size(11.0).color(color(
                ui.ctx(),
                Color32::from_rgb(215, 201, 252),
                Color32::from_rgb(91, 80, 140),
            )));
        });
}

pub(super) fn display_author(author: &str, official: bool) -> &str {
    if official && author == "岛仔" {
        "小唐"
    } else {
        author
    }
}

fn entry_author<'a>(catalog: &market::Catalog, entry: &'a market::Entry) -> &'a str {
    display_author(
        &entry.author,
        catalog.is_official()
            && entry
                .presentation
                .as_ref()
                .is_some_and(|details| details.official),
    )
}

fn entry_badge(ui: &mut egui::Ui, catalog: &market::Catalog, entry: &market::Entry) {
    if let Some(details) = entry
        .presentation
        .as_ref()
        .filter(|_| catalog.is_official())
    {
        show_badge(ui, details.official, details.sample);
    }
}

fn counts(ui: &mut egui::Ui, stats: Option<market_metrics::Counts>) {
    let downloads = stats.as_ref().map(|stats| stats.downloads);
    let favorites = stats.as_ref().map(|stats| stats.favorites);
    let count = |value: Option<u64>| value.map(|n| n.to_string()).unwrap_or_else(|| "—".into());
    ui.horizontal_wrapped(|ui| {
        for (label, value) in [("下载次数", downloads), ("收藏次数", favorites)] {
            ui.label(
                egui::RichText::new(format!("{label} {}", count(value)))
                    .size(11.0)
                    .weak(),
            )
            .on_hover_text(if value.is_some() {
                label
            } else {
                "暂无统计数据"
            });
        }
    });
}

const AVATAR_TEXTURES: &str = "plugin-market-avatar-textures";
type AvatarTextures = std::collections::VecDeque<(egui::Id, egui::TextureHandle)>;

impl LocalPlugins {
    pub(super) fn market(&mut self, ui: &mut egui::Ui, official: bool, favorites: bool) -> bool {
        if official {
            self.ensure_official_catalog();
            if let Some(delay) = self.market_stats_repaint_delay() {
                ui.ctx().request_repaint_after(delay);
            }
        }
        let busy = self.market_busy();
        let syncing = official && self.official_catalog_busy();
        let detail = ui.data(|data| data.get_temp::<String>(detail_id()));
        let back = if favorites {
            "返回我的收藏"
        } else if official {
            "返回插件集市"
        } else {
            "返回本地测试集市"
        };
        let exit = ui
            .horizontal_wrapped(|ui| {
                if detail.is_some() {
                    if ui.button(back).clicked() {
                        close_detail(ui);
                    }
                    return false;
                }
                if !official {
                    ui.strong("本地测试集市");
                    if ui.button("返回我的插件").clicked() {
                        return true;
                    }
                }
                if ui
                    .add_enabled(!busy && !syncing, egui::Button::new("刷新目录"))
                    .clicked()
                {
                    forget_covers(ui);
                    if official {
                        ui.data_mut(|data| {
                            data.remove::<AvatarTextures>(egui::Id::new(AVATAR_TEXTURES));
                        });
                        self.refresh_official_catalog();
                    } else if let Err(error) = self.refresh_market_catalog() {
                        self.notice = error.to_string();
                    }
                }
                false
            })
            .inner;
        if exit {
            return true;
        }
        ui.add_space(6.0);
        if syncing || busy {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(if syncing {
                    "正在同步插件集市…"
                } else {
                    "正在校验插件包…"
                });
            });
            ui.add_space(4.0);
        }
        let catalog = self.market_catalog(official);
        if official && !syncing {
            if let Some(error) = self.official_catalog_error() {
                ui.label(if catalog.is_some() {
                    "同步失败，正在显示上次的集市目录"
                } else {
                    "暂时无法加载集市，请刷新重试"
                })
                .on_hover_text(error);
                ui.add_space(4.0);
            }
        }
        if let Some(id) = ui.data(|data| data.get_temp::<String>(detail_id())) {
            if let Some((catalog, entry)) = catalog.as_ref().and_then(|catalog| {
                catalog
                    .entries
                    .iter()
                    .find(|entry| entry.id == id)
                    .map(|entry| (catalog, entry))
            }) {
                return self.market_details(ui, catalog, entry);
            }
            if !syncing {
                settings_card(ui, |ui| {
                    ui.strong(if catalog.is_some() {
                        "此插件已下架"
                    } else {
                        "暂时无法查看详情"
                    });
                    if favorites && self.is_favorite(&id) && ui.button("取消收藏").clicked() {
                        if let Err(error) = self.remove_favorite(&id) {
                            self.notice = error.to_string();
                        }
                        close_detail(ui);
                    }
                });
            }
            return false;
        }
        if favorites && self.favorites().is_empty() {
            settings_card(ui, |ui| {
                ui.strong("还没有收藏插件");
                settings_hint(ui, "在插件集市收藏后，可以在这里找到它。");
            });
            return false;
        }
        let mut show_mine = false;
        if let Some(catalog) = &catalog {
            let entries: Vec<_> = catalog
                .entries
                .iter()
                .filter(|entry| !favorites || self.is_favorite(&entry.id))
                .collect();
            if entries.is_empty() && !favorites {
                settings_card(ui, |ui| {
                    ui.strong("目录中暂无插件");
                });
            }
            let columns = if ui.available_width() >= 575.0 {
                3
            } else if ui.available_width() >= 370.0 {
                2
            } else {
                1
            };
            for row in entries.chunks(columns) {
                ui.columns(columns, |cells| {
                    for (ui, &entry) in cells.iter_mut().zip(row) {
                        show_mine |= ui
                            .push_id(&entry.id, |ui| self.market_card(ui, catalog, entry))
                            .inner;
                    }
                });
                ui.add_space(8.0);
            }
        }
        if favorites {
            let unavailable: Vec<_> = self
                .favorites()
                .iter()
                .filter(|(id, _)| {
                    catalog
                        .as_ref()
                        .is_none_or(|catalog| !catalog.entries.iter().any(|entry| &entry.id == *id))
                })
                .map(|(id, name)| (id.clone(), name.clone()))
                .collect();
            for (id, name) in unavailable {
                ui.push_id(&id, |ui| {
                    settings_card(ui, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.strong(name);
                            settings_hint(
                                ui,
                                if catalog.is_some() {
                                    "已下架"
                                } else {
                                    "详情暂不可用"
                                },
                            );
                            if ui.button("取消收藏").clicked() {
                                if let Err(error) = self.remove_favorite(&id) {
                                    self.notice = error.to_string();
                                }
                            }
                        });
                    });
                });
            }
        }
        show_mine
    }

    fn market_cover(
        &mut self,
        ui: &mut egui::Ui,
        catalog: &Arc<market::Catalog>,
        entry: &market::Entry,
        radius: egui::CornerRadius,
    ) -> bool {
        let key = egui::Id::new(("market", &catalog.path, &entry.sha256));
        let width = ui.available_width();
        plugin_cover(ui, key, width, radius, "封面暂不可用", || {
            if catalog.is_official() {
                self.official_cover(catalog, entry)
                    .map(|cover| cover.map(|png| png.bytes().to_vec()))
            } else {
                Some(catalog.cover_png(entry).map_err(|_| ()))
            }
        })
    }

    fn market_avatar(
        &mut self,
        ui: &mut egui::Ui,
        catalog: &Arc<market::Catalog>,
        entry: &market::Entry,
        diameter: f32,
    ) {
        let size = egui::vec2(diameter, diameter);
        let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
        if !ui.is_rect_visible(rect) {
            return;
        }
        let avatar = entry
            .presentation
            .as_ref()
            .filter(|_| catalog.is_official())
            .and_then(|details| details.author_avatar.as_deref());
        let texture = avatar.and_then(|filename| {
            let cache_id = egui::Id::new(AVATAR_TEXTURES);
            let key = egui::Id::new((&catalog.path, filename));
            if let Some(texture) = ui.data_mut(|data| {
                let cache = data.get_temp_mut_or_default::<AvatarTextures>(cache_id);
                let index = cache.iter().position(|(id, _)| *id == key)?;
                let entry = cache.remove(index)?;
                let texture = entry.1.clone();
                cache.push_front(entry);
                Some(texture)
            }) {
                return Some(texture);
            }
            let png = self.official_avatar(catalog, entry)?.ok()?;
            let image =
                image::load_from_memory_with_format(png.bytes(), image::ImageFormat::Png).ok()?;
            let edge = image.width().min(image.height());
            let pixels = image
                .crop_imm(
                    (image.width() - edge) / 2,
                    (image.height() - edge) / 2,
                    edge,
                    edge,
                )
                .thumbnail(96, 96)
                .to_rgba8();
            let image = egui::ColorImage::from_rgba_unmultiplied(
                [pixels.width() as usize, pixels.height() as usize],
                pixels.as_raw(),
            );
            let texture =
                ui.ctx()
                    .load_texture("插件作者头像", image, egui::TextureOptions::LINEAR);
            ui.data_mut(|data| {
                let cache = data.get_temp_mut_or_default::<AvatarTextures>(cache_id);
                cache.push_front((key, texture.clone()));
                cache.truncate(24);
            });
            Some(texture)
        });
        if let Some(texture) = texture {
            egui::Image::new(&texture)
                .corner_radius(diameter / 2.0)
                .paint_at(ui, rect);
        } else {
            ui.painter().circle_filled(
                rect.center(),
                diameter / 2.0,
                color(
                    ui.ctx(),
                    Color32::from_rgb(60, 52, 83),
                    Color32::from_rgb(232, 225, 248),
                ),
            );
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                entry_author(catalog, entry)
                    .trim()
                    .chars()
                    .next()
                    .unwrap_or('·'),
                egui::FontId::proportional(diameter * 0.52),
                color(
                    ui.ctx(),
                    Color32::from_rgb(218, 205, 250),
                    Color32::from_rgb(89, 73, 140),
                ),
            );
        }
    }

    fn market_card(
        &mut self,
        ui: &mut egui::Ui,
        catalog: &Arc<market::Catalog>,
        entry: &market::Entry,
    ) -> bool {
        market_frame(ui)
            .show(ui, |ui| {
                let cover = ui.vertical(|ui| {
                    self.market_cover(
                        ui,
                        catalog,
                        entry,
                        egui::CornerRadius {
                            nw: 9,
                            ne: 9,
                            sw: 0,
                            se: 0,
                        },
                    )
                });
                if ui
                    .interact(
                        cover.response.rect,
                        ui.id().with("详情封面"),
                        egui::Sense::click(),
                    )
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked()
                {
                    select_detail(ui, &entry.id);
                }
                egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(12, 10))
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 5.0;
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().button_padding.x = 0.0;
                            let title = egui::RichText::new(&entry.name).size(15.0).strong();
                            if ui
                                .add(egui::Button::new(title).frame(false).truncate())
                                .on_hover_text(&entry.name)
                                .clicked()
                            {
                                select_detail(ui, &entry.id);
                            }
                            entry_badge(ui, catalog, entry);
                        });
                        ui.horizontal(|ui| {
                            let author = entry_author(catalog, entry);
                            self.market_avatar(ui, catalog, entry, 18.0);
                            ui.add(
                                egui::Label::new(egui::RichText::new(author).weak().size(11.0))
                                    .truncate(),
                            )
                            .on_hover_text(author);
                        });
                        if catalog.is_official() {
                            counts(ui, self.market_counts(&entry.id));
                        }
                        ui.add_space(4.0);
                        ui.horizontal_wrapped(|ui| {
                            let manage = self.market_action_button(ui, catalog, entry, cover.inner);
                            if catalog.is_official() {
                                self.market_favorite_button(ui, entry);
                            }
                            manage
                        })
                        .inner
                    })
                    .inner
            })
            .inner
    }

    fn market_details(
        &mut self,
        ui: &mut egui::Ui,
        catalog: &Arc<market::Catalog>,
        entry: &market::Entry,
    ) -> bool {
        market_frame(ui)
            .inner_margin(egui::Margin::same(16))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                let wide = ui.available_width() >= 470.0;
                let intro = |host: &mut Self, ui: &mut egui::Ui, has_cover| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(egui::RichText::new(&entry.name).size(22.0).strong());
                        entry_badge(ui, catalog, entry);
                    });
                    ui.horizontal(|ui| {
                        host.market_avatar(ui, catalog, entry, 28.0);
                        ui.label(egui::RichText::new(entry_author(catalog, entry)).strong());
                    });
                    ui.horizontal_wrapped(|ui| {
                        settings_hint(ui, &format!("版本 v{}", entry.version));
                        if let Some(date) = entry
                            .presentation
                            .as_ref()
                            .and_then(|details| details.updated_on.as_ref())
                        {
                            settings_hint(ui, &format!("更新于 {date}"));
                        }
                    });
                    if catalog.is_official() {
                        counts(ui, host.market_counts(&entry.id));
                    }
                    ui.add_space(5.0);
                    ui.horizontal_wrapped(|ui| {
                        let manage = host.market_action_button(ui, catalog, entry, has_cover);
                        if catalog.is_official() {
                            host.market_favorite_button(ui, entry);
                            if ui.button("反馈").clicked() {
                                request_plugin_feedback(ui, &entry.id, &entry.name, &entry.version);
                            }
                        }
                        manage
                    })
                    .inner
                };
                let radius = egui::CornerRadius::same(8);
                let manage = if wide {
                    ui.columns(2, |columns| {
                        let has_cover = self.market_cover(&mut columns[0], catalog, entry, radius);
                        intro(self, &mut columns[1], has_cover)
                    })
                } else {
                    let has_cover = self.market_cover(ui, catalog, entry, radius);
                    ui.add_space(5.0);
                    intro(self, ui, has_cover)
                };
                ui.add_space(10.0);
                ui.separator();
                let description = entry
                    .presentation
                    .as_ref()
                    .and_then(|details| details.description.as_deref())
                    .unwrap_or("在岛上显示信息。");
                ui.label(description);
                settings_hint(ui, "无需外部权限");
                manage
            })
            .inner
    }

    fn market_favorite_button(&mut self, ui: &mut egui::Ui, entry: &market::Entry) {
        let saved = self.is_favorite(&entry.id);
        let label = if saved { "已收藏" } else { "收藏" };
        if ui
            .add(egui::Button::new(label).selected(saved))
            .on_hover_text(if saved {
                "取消收藏"
            } else {
                "收藏到我的收藏"
            })
            .clicked()
        {
            if let Err(error) = self.set_favorite(entry, !saved) {
                self.notice = error.to_string();
            }
        }
    }

    fn market_action_button(
        &mut self,
        ui: &mut egui::Ui,
        catalog: &market::Catalog,
        entry: &market::Entry,
        has_cover: bool,
    ) -> bool {
        let state = self.market_status(catalog, entry);
        let label = match state {
            MarketStatus::Install => "安装",
            MarketStatus::Update => "更新",
            MarketStatus::Installed => "已安装",
            MarketStatus::OtherSource => {
                settings_hint(ui, "已从其他来源安装");
                return false;
            }
            MarketStatus::Unavailable => {
                settings_hint(ui, "已安装更高版本");
                return false;
            }
        };
        if state == MarketStatus::Update {
            let package = self.records[&entry.id].package.as_ref().unwrap();
            settings_hint(ui, &format!("已安装 v{}", package.manifest.version));
        }
        if state == MarketStatus::Installed {
            return ui.button(label).clicked();
        }
        if ui
            .add_enabled(!self.market_busy() && has_cover, market_button(ui, label))
            .clicked()
        {
            self.notice = self
                .market_action(catalog.is_official(), &entry.id)
                .err()
                .map(|error| error.to_string())
                .unwrap_or_default();
            ui.ctx().request_repaint();
        }
        false
    }
}
