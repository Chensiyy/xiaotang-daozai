use super::*;

fn detail_id() -> egui::Id {
    egui::Id::new("plugin-market-details")
}

fn favorites_only_id() -> egui::Id {
    egui::Id::new("plugin-market-favorites-only")
}

pub(super) fn select_detail(ui: &egui::Ui, id: &str) {
    let now = ui.input(|input| input.time);
    ui.data_mut(|data| {
        data.insert_temp(detail_id(), id.to_owned());
        data.insert_temp(detail_id().with("opened"), now);
    });
    ui.ctx().request_repaint();
}

pub(super) fn close_detail(ui: &egui::Ui) {
    ui.data_mut(|data| data.remove::<String>(detail_id()));
}

/// 集市切换的结果：留在原处、去「已安装」，或切换官方与本地测试目录。
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum MarketNav {
    Stay,
    Mine,
    Source { official: bool },
}

fn market_frame(ui: &egui::Ui) -> egui::Frame {
    egui::Frame::new()
        .fill(color(
            ui.ctx(),
            Color32::from_rgb(35, 35, 38),
            Color32::WHITE,
        ))
        .stroke(egui::Stroke::new(1.0, line_color(ui)))
        .corner_radius(10.0)
}

pub(super) fn show_badge(ui: &mut egui::Ui, official: bool, sample: bool) {
    let label = match (official, sample) {
        (true, _) => "官方",
        (false, true) => "验收样例",
        _ => return,
    };
    small_tag(ui, label);
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

fn entry_description<'a>(entry: &'a market::Entry) -> &'a str {
    entry
        .presentation
        .as_ref()
        .and_then(|details| details.description.as_deref())
        .unwrap_or("在待办与置物中显示信息卡片。")
}

fn compact(value: Option<u64>) -> String {
    match value {
        None => "—".into(),
        Some(n) if n >= 10_000 => format!("{:.1}万", n as f64 / 10_000.0),
        Some(n) => n.to_string(),
    }
}

/// 收藏用的心形图标：收藏后变红，不配文字。
fn heart(ui: &mut egui::Ui, id: &str, saved: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(24.0, 24.0), egui::Sense::click());
    let label = if saved { "取消收藏" } else { "收藏" };
    response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, saved, label));
    let ctx = ui.ctx();
    let filled = ctx.animate_bool_with_time(egui::Id::new(("plugin-heart", id)), saved, 0.18);
    let hover = ctx.animate_bool_with_time(response.id, response.hovered(), 0.12);
    let idle = paint::mix(muted_color(ui), plugin_accent(ui), hover * 0.6);
    let red = color(ctx, Color32::from_rgb(240, 98, 120), Color32::from_rgb(214, 69, 93));
    let tint = paint::mix(idle, red, filled);
    // 被点中时轻轻放大一下。
    let scale = 1.0 + 0.18 * (filled * (1.0 - filled) * 4.0);
    let c = rect.center();
    let s = 1.0 * scale;
    let painter = ui.painter();
    painter.circle_filled(egui::pos2(c.x - 3.3 * s, c.y - 1.6 * s), 3.7 * s, tint);
    painter.circle_filled(egui::pos2(c.x + 3.3 * s, c.y - 1.6 * s), 3.7 * s, tint);
    painter.add(egui::Shape::convex_polygon(
        vec![
            egui::pos2(c.x - 6.75 * s, c.y - 0.6 * s),
            egui::pos2(c.x + 6.75 * s, c.y - 0.6 * s),
            egui::pos2(c.x, c.y + 6.4 * s),
        ],
        tint,
        egui::Stroke::NONE,
    ));
    if response.has_focus() {
        painter.rect_stroke(
            rect,
            6.0,
            egui::Stroke::new(1.0, plugin_accent(ui)),
            egui::StrokeKind::Inside,
        );
    }
    response.on_hover_text(label)
}

/// 刷新目录按钮：同步时图标自己转，失败时边框变琥珀色呼吸，悬停看原因。
fn refresh_button(ui: &mut egui::Ui, enabled: bool, spinning: bool, failure: Option<&str>) -> egui::Response {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(30.0, 26.0), egui::Sense::hover());
    let response = ui.interact(rect, egui::Id::new("plugin-market-refresh"), if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    });
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, "刷新目录"));
    let ctx = ui.ctx().clone();
    let time = ui.input(|input| input.time) as f32;
    let amber = warn_color(ui);
    let mut stroke = line_color(ui);
    if failure.is_some() && !spinning {
        let phase = if crate::bar_layout::animations_enabled() {
            ctx.request_repaint_after(Duration::from_millis(60));
            0.5 + 0.5 * (time * std::f32::consts::TAU * 0.6).sin()
        } else {
            1.0
        };
        stroke = paint::mix(line_color(ui), amber, 0.55 + 0.45 * phase);
    }
    let fill = if response.hovered() && enabled {
        color(&ctx, Color32::from_rgb(48, 48, 54), Color32::from_rgb(244, 245, 248))
    } else {
        card_color(ui)
    };
    ui.painter().rect(rect, 7.0, fill, egui::Stroke::new(1.0, stroke), egui::StrokeKind::Inside);
    let icon = if spinning {
        plugin_accent(ui)
    } else if failure.is_some() {
        amber
    } else {
        muted_color(ui)
    };
    let rotation = if spinning && crate::bar_layout::animations_enabled() {
        ctx.request_repaint();
        time * std::f32::consts::TAU
    } else {
        0.0
    };
    let c = rect.center();
    let r = 5.8;
    let start = -0.35 + rotation;
    let sweep = std::f32::consts::TAU * 0.78;
    let points: Vec<_> = (0..=18)
        .map(|i| {
            let a = start + sweep * i as f32 / 18.0;
            egui::pos2(c.x + r * a.cos(), c.y + r * a.sin())
        })
        .collect();
    let end = start + sweep;
    let tip = egui::pos2(c.x + r * end.cos(), c.y + r * end.sin());
    let tangent = egui::vec2(-end.sin(), end.cos());
    let normal = egui::vec2(end.cos(), end.sin());
    ui.painter().add(egui::Shape::line(points, egui::Stroke::new(1.6, icon)));
    ui.painter().add(egui::Shape::convex_polygon(
        vec![
            tip + tangent * 2.6,
            tip - normal * 2.8 - tangent * 1.2,
            tip + normal * 2.8 - tangent * 1.2,
        ],
        icon,
        egui::Stroke::NONE,
    ));
    match failure {
        Some(error) if !spinning => response.on_hover_text(format!("同步失败，正在显示上次的集市目录。点一下重试\n{error}")),
        _ => response.on_hover_text("刷新目录"),
    }
}

const AVATAR_TEXTURES: &str = "plugin-market-avatar-textures";
type AvatarTextures = std::collections::VecDeque<(egui::Id, egui::TextureHandle)>;

impl LocalPlugins {
    /// 集市页：上方一行筛选和刷新，下面是插件卡片；点开卡片在右侧抽屉看详情。
    pub(super) fn market(&mut self, ui: &mut egui::Ui, official: bool) -> MarketNav {
        if official {
            self.ensure_official_catalog();
            if let Some(delay) = self.market_stats_repaint_delay() {
                ui.ctx().request_repaint_after(delay);
            }
        }
        let busy = self.market_busy();
        let syncing = official && self.official_catalog_busy();
        let catalog = self.market_catalog(official);
        let failure = if official && !syncing {
            self.official_catalog_error().map(str::to_owned)
        } else {
            None
        };
        let has_test = self.market_catalog(false).is_some();
        let favorites_count = self.favorites().len();
        let mut favorites = official && ui.data(|data| data.get_temp::<bool>(favorites_only_id())).unwrap_or(false);
        let mut nav = MarketNav::Stay;

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            if official {
                if chip(ui, "全部", !favorites).clicked() {
                    favorites = false;
                }
                if chip(ui, &format!("收藏 {favorites_count}"), favorites).clicked() {
                    favorites = true;
                }
            } else {
                ui.label(egui::RichText::new("本地测试集市").strong());
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if refresh_button(ui, !busy && !syncing, syncing, failure.as_deref()).clicked() {
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
                if has_test {
                    let options = ["官方".to_owned(), "本地测试".to_owned()];
                    if let Some(index) = segmented(ui, "plugin-market-source", &options, usize::from(!official), &[false, false], 12.0) {
                        nav = MarketNav::Source { official: index == 0 };
                    }
                }
            });
        });
        ui.data_mut(|data| data.insert_temp(favorites_only_id(), favorites));
        if nav != MarketNav::Stay {
            close_detail(ui);
            return nav;
        }
        ui.add_space(10.0);

        if catalog.is_none() {
            settings_card(ui, |ui| {
                if syncing {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        settings_hint(ui, "正在同步插件集市…");
                    });
                } else {
                    ui.strong("暂时无法加载集市，请刷新重试");
                }
            });
        }
        if favorites && favorites_count == 0 {
            settings_card(ui, |ui| {
                ui.strong("还没有收藏插件");
                settings_hint(ui, "点卡片上的心形收藏，之后在这里找到它。");
            });
        }
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
                        if ui.push_id(&entry.id, |ui| self.market_card(ui, catalog, entry)).inner {
                            nav = MarketNav::Mine;
                        }
                    }
                });
                ui.add_space(10.0);
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
        if let Some(id) = ui.data(|data| data.get_temp::<String>(detail_id())) {
            let found = catalog.as_ref().and_then(|catalog| {
                catalog
                    .entries
                    .iter()
                    .find(|entry| entry.id == id)
                    .map(|entry| (Arc::clone(catalog), entry.clone()))
            });
            if self.market_drawer(ui, &id, found, favorites) {
                nav = MarketNav::Mine;
            }
        }
        if nav == MarketNav::Mine {
            close_detail(ui);
        }
        nav
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
        plugin_cover(ui, key, width, radius, "封面暂不可用", false, || {
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

    /// 集市卡片：封面、名称、作者、两行简介、装到哪里，底部是下载量、收藏和安装。
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
                        ui.horizontal(|ui| {
                            ui.spacing_mut().button_padding.x = 0.0;
                            let title = egui::RichText::new(&entry.name).size(14.5).strong();
                            let badge_w = if entry.presentation.as_ref().is_some_and(|p| p.official || p.sample) { 64.0 } else { 0.0 };
                            ui.set_max_width(ui.available_width());
                            let name_w = (ui.available_width() - badge_w).max(40.0);
                            ui.scope(|ui| {
                                ui.set_max_width(name_w);
                                if ui
                                    .add(egui::Button::new(title).frame(false).truncate())
                                    .on_hover_text(&entry.name)
                                    .clicked()
                                {
                                    select_detail(ui, &entry.id);
                                }
                            });
                            entry_badge(ui, catalog, entry);
                        });
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;
                            let author = format!("{} · v{}", entry_author(catalog, entry), entry.version);
                            self.market_avatar(ui, catalog, entry, 16.0);
                            ui.add(
                                egui::Label::new(egui::RichText::new(&author).size(11.0).color(muted_color(ui)))
                                    .truncate(),
                            )
                            .on_hover_text(author);
                        });
                        clamped_text(ui, entry_description(entry), 12.0, 2, muted_color(ui));
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new("装到 待办与置物").size(11.0).color(muted_color(ui)),
                            )
                            .truncate(),
                        );
                        ui.add_space(2.0);
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;
                            if catalog.is_official() {
                                let downloads = self.market_counts(&entry.id).map(|counts| counts.downloads);
                                ui.label(
                                    egui::RichText::new(format!("下载 {}", compact(downloads)))
                                        .size(11.0)
                                        .color(muted_color(ui)),
                                )
                                .on_hover_text(if downloads.is_some() { "下载次数" } else { "暂无统计数据" });
                            }
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
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
            })
            .inner
    }

    /// 右侧详情抽屉：不离开列表，固定写清出现在哪里、设置在哪、要什么权限。
    fn market_drawer(
        &mut self,
        ui: &mut egui::Ui,
        id: &str,
        found: Option<(Arc<market::Catalog>, market::Entry)>,
        favorites: bool,
    ) -> bool {
        let ctx = ui.ctx().clone();
        let clip = ui.clip_rect();
        let opened = ui
            .data(|data| data.get_temp::<f64>(detail_id().with("opened")))
            .unwrap_or_default();
        let elapsed = (ui.input(|input| input.time) - opened) as f32;
        let t = if crate::bar_layout::animations_enabled() {
            let t = (elapsed / 0.2).clamp(0.0, 1.0);
            if t < 1.0 {
                ctx.request_repaint();
            }
            1.0 - (1.0 - t).powi(3)
        } else {
            1.0
        };
        let width = (clip.width() * 0.92).min(440.0);
        let mut close = ctx.input(|input| input.key_pressed(egui::Key::Escape));
        let scrim = egui::Area::new(egui::Id::new("plugin-detail-scrim"))
            .order(egui::Order::Middle)
            .fixed_pos(clip.min)
            .show(&ctx, |ui| {
                let (rect, response) = ui.allocate_exact_size(clip.size(), egui::Sense::click());
                ui.painter()
                    .rect_filled(rect, 0.0, Color32::from_black_alpha((90.0 * t) as u8));
                response.clicked()
            });
        close |= scrim.inner;
        let left = clip.right() - width * t;
        let mut manage = false;
        egui::Area::new(egui::Id::new("plugin-detail-drawer"))
            .order(egui::Order::Foreground)
            .fixed_pos(egui::pos2(left, clip.top()))
            .show(&ctx, |ui| {
                egui::Frame::new()
                    .fill(color(&ctx, Color32::from_rgb(30, 30, 34), Color32::WHITE))
                    .stroke(egui::Stroke::new(1.0, line_color(ui)))
                    .inner_margin(egui::Margin::same(18))
                    .show(ui, |ui| {
                        ui.set_width(width - 36.0);
                        ui.set_height(clip.height() - 36.0);
                        settings_apply_style(ui);
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(id).size(11.0).color(muted_color(ui)));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                close |= ui
                                    .add(egui::Button::new(egui::RichText::new("×").size(18.0)).frame(false))
                                    .on_hover_text("关闭")
                                    .clicked();
                            });
                        });
                        egui::ScrollArea::vertical()
                            .id_salt("plugin-detail-scroll")
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                ui.spacing_mut().item_spacing.y = 8.0;
                                match &found {
                                    Some((catalog, entry)) => {
                                        manage = self.market_details(ui, catalog, entry);
                                    }
                                    None => {
                                        ui.strong(if self.market_catalog(true).is_some() || !favorites {
                                            "此插件已下架"
                                        } else {
                                            "暂时无法查看详情"
                                        });
                                        if self.is_favorite(id) && ui.button("取消收藏").clicked() {
                                            if let Err(error) = self.remove_favorite(id) {
                                                self.notice = error.to_string();
                                            }
                                            close = true;
                                        }
                                    }
                                }
                            });
                    });
            });
        if close {
            close_detail(ui);
        }
        manage
    }

    fn market_details(
        &mut self,
        ui: &mut egui::Ui,
        catalog: &Arc<market::Catalog>,
        entry: &market::Entry,
    ) -> bool {
        let has_cover = self.market_cover(ui, catalog, entry, egui::CornerRadius::same(9));
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new(&entry.name).size(19.0).strong());
            entry_badge(ui, catalog, entry);
        });
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            self.market_avatar(ui, catalog, entry, 20.0);
            let mut meta = format!("{} · 版本 v{}", entry_author(catalog, entry), entry.version);
            if let Some(date) = entry
                .presentation
                .as_ref()
                .and_then(|details| details.updated_on.as_ref())
            {
                meta.push_str(&format!(" · 更新于 {date}"));
            }
            if catalog.is_official() {
                let counts = self.market_counts(&entry.id);
                meta.push_str(&format!(
                    " · 下载 {} · 收藏 {}",
                    compact(counts.as_ref().map(|c| c.downloads)),
                    compact(counts.as_ref().map(|c| c.favorites))
                ));
            }
            ui.label(egui::RichText::new(meta).size(11.5).color(muted_color(ui)));
        });
        ui.label(entry_description(entry));
        egui::Frame::new()
            .fill(color(ui.ctx(), Color32::from_rgb(38, 38, 43), Color32::from_rgb(245, 246, 249)))
            .corner_radius(9.0)
            .inner_margin(egui::Margin::same(12))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.spacing_mut().item_spacing.y = 6.0;
                for (label, value) in [
                    ("出现在", "待办与置物里的信息卡片"),
                    ("设置在", "设置 › 待办与置物"),
                    ("新增页面", "不会"),
                    ("权限", "无需外部权限"),
                ] {
                    ui.horizontal(|ui| {
                        let (rect, _) = ui.allocate_exact_size(egui::vec2(64.0, 16.0), egui::Sense::hover());
                        ui.painter().text(
                            rect.left_center(),
                            egui::Align2::LEFT_CENTER,
                            label,
                            egui::FontId::proportional(12.0),
                            muted_color(ui),
                        );
                        ui.add(egui::Label::new(egui::RichText::new(value).size(12.5)).wrap());
                    });
                }
            });
        if self.market_status(catalog, entry) == MarketStatus::Update {
            let package = self.records[&entry.id].package.as_ref().unwrap();
            settings_hint(ui, &format!("已安装 v{}", package.manifest.version));
        }
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            let manage = self.market_action_button(ui, catalog, entry, has_cover);
            if catalog.is_official() {
                self.market_favorite_button(ui, entry);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if catalog.is_official() && ui.add(egui::Button::new("反馈问题").frame(false)).clicked() {
                    request_plugin_feedback(ui, &entry.id, &entry.name, &entry.version);
                }
            });
            manage
        })
        .inner
    }

    fn market_favorite_button(&mut self, ui: &mut egui::Ui, entry: &market::Entry) {
        let saved = self.is_favorite(&entry.id);
        if heart(ui, &entry.id, saved).clicked() {
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
        if self.market_job_id() == Some(entry.id.as_str()) {
            // 安装中：按钮保持原位，文字换成转圈。
            let label = if state == MarketStatus::Update { "更新" } else { "安装" };
            let button = egui::Button::new(egui::RichText::new(label).color(Color32::TRANSPARENT))
                .fill(plugin_accent(ui))
                .corner_radius(6.0);
            let response = ui.add_enabled(false, button);
            let size = response.rect.height() * 0.55;
            egui::Spinner::new()
                .size(size)
                .color(Color32::WHITE)
                .paint_at(ui, egui::Rect::from_center_size(response.rect.center(), egui::vec2(size, size)));
            ui.ctx().request_repaint();
            return false;
        }
        let label = match state {
            MarketStatus::Install => "安装",
            MarketStatus::Update => "更新",
            MarketStatus::Installed => {
                let green = ok_color(ui);
                let response = ui
                    .add(
                        egui::Button::new(egui::RichText::new("已安装").color(green))
                            .fill(color(ui.ctx(), Color32::from_rgb(31, 53, 39), Color32::from_rgb(227, 243, 232)))
                            .stroke(egui::Stroke::NONE)
                            .corner_radius(6.0),
                    )
                    .on_hover_text("在已安装里管理");
                return response.clicked();
            }
            MarketStatus::OtherSource => {
                settings_hint(ui, "已从其他来源安装");
                return false;
            }
            MarketStatus::Unavailable => {
                settings_hint(ui, "已安装更高版本");
                return false;
            }
        };
        let mut response = ui.add_enabled(!self.market_busy() && has_cover, market_button(ui, label));
        if state == MarketStatus::Update {
            let installed = &self.records[&entry.id].package.as_ref().unwrap().manifest.version;
            response = response.on_hover_text(format!("已安装 v{installed}，更新到 v{}", entry.version));
        }
        if response.clicked() {
            self.notice = self
                .market_action(catalog.is_official(), &entry.id)
                .err()
                .map(|error| error.to_string())
                .unwrap_or_default();
            ui.ctx().request_repaint();
        }
        false
    }

    /// 已安装插件在集市里有新版本时返回新版本号。
    pub(super) fn market_update(&self, id: &str) -> Option<(bool, String)> {
        let record = self.records.get(id)?;
        if !record.from_market {
            return None;
        }
        let catalog = self.market_catalog(record.official_market)?;
        let entry = catalog.entries.iter().find(|entry| entry.id == id)?;
        (self.market_status(&catalog, entry) == MarketStatus::Update)
            .then(|| (record.official_market, entry.version.clone()))
    }
}
