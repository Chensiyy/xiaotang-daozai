use super::{egui, plugin_switch, settings_control_row, Field, Values};
use crate::extras::{settings_segmented, time_stepper};
use chrono::Timelike;

pub(super) fn field_row(ui: &mut egui::Ui, plugin: &str, field: &Field, value: &mut String) {
    ui.push_id(&field.key, |ui| {
        if field.kind == "list" {
            list_field(ui, plugin, field, value);
        } else {
            settings_control_row(ui, &field.label, 68.0, |ui| {
                ui.scope(|ui| editor(ui, plugin, field, value)).response
            });
        }
    });
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

fn list_field(ui: &mut egui::Ui, plugin: &str, field: &Field, value: &mut String) {
    let children = field.item.as_ref().unwrap();
    let mut rows: Vec<Values> = serde_json::from_str(value).unwrap_or_default();
    let mut edit = None;
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
                    field_row(ui, plugin, child, row.get_mut(&child.key).unwrap());
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
}
