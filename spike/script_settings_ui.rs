use super::{egui, plugin_switch, settings_control_row, Field, Values};
use crate::{
    extras::{settings_segmented, time_stepper},
    plugin_setting_catalog::{setting_color, timezone_cities, COLORS, ICONS},
};
use chrono::Timelike;

pub(super) fn field_row(ui: &mut egui::Ui, plugin: &str, field: &Field, value: &mut String) -> bool {
    ui.push_id(&field.key, |ui| {
        if field.kind == "list" {
            list_field(ui, plugin, field, value)
        } else {
            let mut submit = false;
            settings_control_row(ui, &field.label, 68.0, |ui| {
                ui.scope(|ui| {
                    if field.multiline == Some(true) {
                        submit = multiline_text(ui, value);
                    } else {
                        editor(ui, plugin, field, value);
                    }
                }).response
            });
            submit
        }
    }).inner
}

fn editor(ui: &mut egui::Ui, plugin: &str, field: &Field, value: &mut String) {
    match field.kind.as_str() {
        "switch" => {
            let on = value == "true";
            if plugin_switch(ui, &format!("{plugin}:{}", field.key), on, &field.label) {
                *value = (!on).to_string();
            }
        }
        "choice" => {
            let choices = field.choices.as_ref().unwrap();
            let options = choices
                .iter()
                .map(|choice| (choice.value.as_str(), choice.label.as_str()))
                .collect::<Vec<_>>();
            let mut selected = value.as_str();
            if options.len() <= 4 {
                settings_segmented(ui, &mut selected, &options, 24.0);
            } else {
                let label = options
                    .iter()
                    .find_map(|(key, label)| (*key == selected).then_some(*label));
                egui::ComboBox::from_id_salt(&field.key)
                    .selected_text(label.unwrap_or_default())
                    .show_ui(ui, |ui| {
                        for (key, label) in options {
                            ui.selectable_value(&mut selected, key, label);
                        }
                    });
            }
            if selected != value.as_str() {
                *value = selected.to_owned();
            }
        }
        "number" => {
            let min = field.min.unwrap();
            let mut number = value.parse::<f32>().unwrap_or(min);
            let mut control = egui::DragValue::new(&mut number)
                .range(min..=field.max.unwrap())
                .speed(field.step.unwrap())
                .suffix(field.unit.as_deref().unwrap_or_default());
            if let Some(precision) = field.precision {
                control = control.fixed_decimals(precision.into());
            }
            if ui.add(control).changed() {
                *value = number.to_string();
            }
        }
        "time" => {
            let time = chrono::NaiveTime::parse_from_str(value, "%H:%M").unwrap_or_default();
            let mut minutes = time.hour() * 60 + time.minute();
            if time_stepper(ui, &mut minutes, &field.key) {
                *value = format!("{:02}:{:02}", minutes / 60, minutes % 60);
            }
        }
        "timezone" => timezone_picker(ui, &field.key, value),
        "icon" => {
            let selected = ICONS.iter().find(|icon| icon.name == value.as_str()).map_or("", |icon| icon.label);
            egui::ComboBox::from_id_salt(&field.key).selected_text(selected).show_ui(ui, |ui| {
                for icon in ICONS {
                    ui.horizontal(|ui| {
                        let svg = format!(
                            r##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24"><path d="{}" fill="none" stroke="#a0a7b0" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"/></svg>"##,
                            icon.path,
                        );
                        ui.add(egui::Image::from_bytes(
                            format!("bytes://plugin-setting-{}.svg", icon.name),
                            svg.into_bytes(),
                        ).fit_to_exact_size(egui::vec2(18.0, 18.0)));
                        if ui.selectable_label(value == icon.name, icon.label).clicked() {
                            *value = icon.name.to_owned();
                        }
                    });
                }
            });
        }
        "color" => {
            let selected = setting_color(value).map_or("", |color| color.label);
            egui::ComboBox::from_id_salt(&field.key).selected_text(selected).show_ui(ui, |ui| {
                for color in COLORS {
                    ui.horizontal(|ui| {
                        let (swatch, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
                        let [r, g, b] = color.rgb;
                        ui.painter().circle_filled(swatch.center(), 5.0, egui::Color32::from_rgb(r, g, b));
                        if ui.selectable_label(value == color.name, color.label).clicked() {
                            *value = color.name.to_owned();
                        }
                    });
                }
            });
        }
        "weekdays" => {
            let mut days = super::script_settings::weekdays(value);
            ui.horizontal_wrapped(|ui| {
                for (index, caption) in ["一", "二", "三", "四", "五", "六", "日"]
                    .into_iter()
                    .enumerate()
                {
                    let day = index as u8 + 1;
                    let on = days.contains(&day);
                    let button = egui::Button::new(caption)
                        .selected(on)
                        .min_size(egui::vec2(24.0, 24.0))
                        .corner_radius(12.0);
                    if ui
                        .add(button)
                        .on_hover_text(format!("星期{caption}"))
                        .clicked()
                    {
                        if on {
                            days.remove(&day);
                        } else {
                            days.insert(day);
                        }
                        *value = days.iter().map(u8::to_string).collect::<Vec<_>>().join(",");
                    }
                }
            });
        }
        _ => {
            ui.text_edit_singleline(value);
        }
    }
}

fn timezone_picker(ui: &mut egui::Ui, key: &str, value: &mut String) {
    let search_id = ui.make_persistent_id(("timezone-search", key));
    egui::ComboBox::from_id_salt(key)
        .selected_text(value.as_str())
        .width(210.0)
        .close_behavior(egui::popup::PopupCloseBehavior::CloseOnClickOutside)
        .show_ui(ui, |ui| {
            let mut query = ui.data(|data| data.get_temp::<String>(search_id)).unwrap_or_default();
            if ui.add(egui::TextEdit::singleline(&mut query)
                .hint_text("搜索城市、国家或时区")
                .desired_width(220.0)).changed() {
                ui.data_mut(|data| data.insert_temp(search_id, query.clone()));
            }
            let matches: Vec<_> = timezone_cities(&query).collect();
            if matches.is_empty() {
                ui.weak("没有符合条件的时区");
            }
            let row_height = ui.spacing().interact_size.y
                .max(ui.text_style_height(&egui::TextStyle::Body) + ui.spacing().button_padding.y * 2.0);
            egui::ScrollArea::vertical().id_salt(search_id).max_height(170.0)
                .show_rows(ui, row_height, matches.len(), |ui, rows| {
                for city in &matches[rows] {
                    let label = format!("{} · {} · {}", city.city, city.region, city.zone);
                    if ui.selectable_label(value == city.zone, label).clicked() {
                        *value = city.zone.to_owned();
                        ui.data_mut(|data| data.remove::<String>(search_id));
                        ui.memory_mut(|memory| memory.close_popup());
                    }
                }
            });
        });
}

fn multiline_text(ui: &mut egui::Ui, value: &mut String) -> bool {
    let id = ui.make_persistent_id("multiline");
    let submit = ui.memory(|memory| memory.has_focus(id))
        && ui.input_mut(|input| input.consume_key(egui::Modifiers::CTRL, egui::Key::Enter));
    let width = ui.available_width().min(370.0);
    let font = egui::TextStyle::Body.resolve(ui.style());
    let color = ui.visuals().text_color();
    let max_rows = 6;
    let rows = ui.fonts(|fonts| fonts.layout(value.clone(), font.clone(), color, (width - 8.0).max(1.0)).rows.len());
    let row_height = ui.text_style_height(&egui::TextStyle::Body);
    let height = row_height * rows.clamp(1, max_rows) as f32 + 8.0;
    let response = egui::ScrollArea::vertical().id_salt(id).max_height(height).auto_shrink([true, false])
        .show(ui, |ui| ui.add(egui::TextEdit::multiline(value).id(id).font(font).desired_width(width).desired_rows(1).char_limit(80)))
        .inner;
    if response.changed() {
        value.retain(|ch| ch == '\n' || !ch.is_control());
    }
    submit
}

fn list_field(ui: &mut egui::Ui, plugin: &str, field: &Field, value: &mut String) -> bool {
    let children = field.item.as_ref().unwrap();
    let mut rows: Vec<Values> = serde_json::from_str(value).unwrap_or_default();
    let mut edit = None;
    let mut submit = false;
    let last = rows.len().saturating_sub(1);
    ui.horizontal(|ui| {
        ui.label(&field.label);
        if ui
            .add_enabled(
                rows.len() < field.max_items.unwrap(),
                egui::Button::new("添加一行"),
            )
            .clicked()
        {
            rows.push(
                children
                    .iter()
                    .map(|child| (child.key.clone(), child.default.clone()))
                    .collect(),
            );
        }
    });
    for (i, row) in rows.iter_mut().enumerate() {
        ui.push_id(i, |ui| {
            ui.group(|ui| {
                ui.horizontal(|ui| {
                    if ui.add_enabled(i > 0, egui::Button::new("上移")).clicked() {
                        edit = Some((i, Some(i - 1)));
                    }
                    if ui
                        .add_enabled(i < last, egui::Button::new("下移"))
                        .clicked()
                    {
                        edit = Some((i, Some(i + 1)));
                    }
                    if ui.button("删除").clicked() {
                        edit = Some((i, None));
                    }
                });
                for child in children {
                    submit |= field_row(ui, plugin, child, row.get_mut(&child.key).unwrap());
                }
            });
        });
    }
    if let Some((i, target)) = edit {
        if let Some(target) = target {
            rows.swap(i, target);
        } else {
            rows.remove(i);
        }
    }
    if let Ok(encoded) = serde_json::to_string(&rows) {
        *value = encoded;
    }
    submit
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multiline_settings_preserve_enter_and_submit_on_ctrl_enter() {
        let ctx = egui::Context::default();
        let mut value = String::new();
        let enter = |modifiers| egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        };
        let frame = |events, value: &mut String| {
            let mut submitted = false;
            let _ = ctx.run(egui::RawInput { events, ..Default::default() }, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let id = ui.make_persistent_id("multiline");
                    ui.memory_mut(|memory| memory.request_focus(id));
                    submitted |= multiline_text(ui, value);
                });
            });
            submitted
        };
        assert!(!frame(vec![], &mut value));
        assert!(!frame(vec![egui::Event::Text("上".into()), enter(egui::Modifiers::NONE), egui::Event::Text("下".into())], &mut value));
        assert_eq!(value, "上\n下");
        assert!(frame(vec![enter(egui::Modifiers::CTRL)], &mut value));
        assert_eq!(value, "上\n下");
    }
}
