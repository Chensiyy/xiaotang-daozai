use std::collections::BTreeMap;

use eframe::egui;

use crate::{
    paint::{self, Card, H},
    plugin_status::{PluginStatus, StatusField, StatusItem},
    theme::{self, size, W},
};

pub(super) const MAX_FIELDS: usize = 3;
pub(super) const ACTION_HEIGHT: f32 = 26.0;
const FIELD_HEIGHT: f32 = 31.0;
const MULTILINE_ROW_HEIGHT: f32 = 15.0;
pub(crate) const DEFAULT_FORM_HEIGHT: f32 = FIELD_HEIGHT * MAX_FIELDS as f32 + ACTION_HEIGHT;

fn field_extra_height(field: &StatusField, text: &str) -> f32 {
    if !field.multiline {
        return 0.0;
    }
    // 状态页字段使用固定内容宽度，中文占两个半角字符的宽度。
    let rows: usize = text.split('\n').map(|line| {
        line.chars().map(|ch| if ch.is_ascii() { 1usize } else { 2 }).sum::<usize>()
            .div_ceil(36).max(1)
    }).sum();
    (rows.clamp(1, 6) - 1) as f32 * MULTILINE_ROW_HEIGHT
}

pub(crate) fn extra_fields_height(status: &PluginStatus) -> f32 {
    status.fields.iter().map(|field| {
        field_extra_height(field, field.value.as_deref().unwrap_or_default())
    }).sum()
}

pub(super) fn fields_height(status: Option<&PluginStatus>) -> f32 {
    status.map_or(FIELD_HEIGHT * MAX_FIELDS as f32, |status| {
        FIELD_HEIGHT * status.fields.len() as f32 + extra_fields_height(status)
    })
}

#[derive(Default)]
pub(crate) struct InteractionResponse {
    pub(crate) focused: bool,
    pub(crate) gained_focus: bool,
    pub(crate) canvas_animating: bool,
    pub(crate) canvas_scroll: Option<egui::Rect>,
    pub(crate) canvas_repaint: Option<std::time::Duration>,
}

#[derive(Clone)]
pub(crate) enum ClickTarget {
    Button(usize),
    Item(usize),
    CopyItem(usize),
    Canvas(crate::plugin_canvas::live::Hit),
}

pub(crate) struct Interaction<'a> {
    plugin: &'a str,
    revision: u64,
    form: &'a mut BTreeMap<String, String>,
    enabled: bool,
    clicked: Option<ClickTarget>,
    response: InteractionResponse,
}

impl<'a> Interaction<'a> {
    pub(crate) fn new(
        plugin: &'a str,
        revision: u64,
        form: &'a mut BTreeMap<String, String>,
        enabled: bool,
    ) -> Self {
        Self {
            plugin,
            revision,
            form,
            enabled,
            clicked: None,
            response: Default::default(),
        }
    }

    pub(crate) fn finish(self) -> (InteractionResponse, Option<ClickTarget>) {
        (self.response, self.clicked)
    }

    pub(super) fn actions(&mut self, c: &Card, status: &PluginStatus, x: f32, y: f32, width: f32) {
        let gap = 6.0;
        let width = ((width + gap) / status.actions.len().max(1) as f32 - gap).min(98.0);
        for (index, action) in status.actions.iter().enumerate() {
            let x = x + index as f32 * (width + gap);
            let card = if self.enabled || action.open.is_some() || action.copy.is_some() {
                c.clone()
            } else {
                c.ghost().faded(0.65)
            };
            let id = format!(
                "plugin-action:{}:{}:{}",
                self.plugin, self.revision, action.id
            );
            let (button, body) = paint::capsule(
                &card,
                x,
                y,
                width,
                ACTION_HEIGHT,
                &id,
                theme::BTN,
                theme::BTN_HOVER,
            );
            let label = body.fit(&action.label, size::SMALL, W::Semibold, width - 12.0);
            body.centered_text(
                x + width / 2.0,
                y + ACTION_HEIGHT / 2.0,
                &label,
                size::SMALL,
                W::Semibold,
                theme::WHITE,
                H::Center,
            );
            if button.clicked() {
                self.clicked = Some(ClickTarget::Button(index));
            }
        }
    }

    pub(super) fn list_target(
        &mut self,
        c: &Card,
        item: &StatusItem,
        index: usize,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) -> f32 {
        let mut body_width = width;
        let key = (self.plugin, self.revision, index);
        if item.copy.is_some() && item.action.is_some() {
            let copy_width = 44.0;
            body_width -= copy_width + 6.0;
            let copy_x = x + width - copy_width;
            let (button, body) = paint::capsule(
                c,
                copy_x,
                y + 1.0,
                copy_width,
                height - 2.0,
                &format!("plugin-list-copy:{key:?}"),
                theme::BTN,
                theme::BTN_HOVER,
            );
            body.centered_text(
                copy_x + copy_width / 2.0,
                y + height / 2.0,
                "复制",
                size::TINY,
                W::Semibold,
                theme::WHITE,
                H::Center,
            );
            if button.clicked() {
                self.clicked = Some(ClickTarget::CopyItem(index));
            }
        }
        if item.open.is_some()
            || (item.copy.is_some() && item.action.is_none())
            || (self.enabled && item.action.is_some())
        {
            let hit = c.hit(x, y, body_width, height, ("plugin-list-item", key));
            if hit.hovered() && c.live {
                c.fill(x, y, body_width, height, 6.0, theme::BTN);
            }
            if hit.clicked() {
                self.clicked = Some(ClickTarget::Item(index));
            }
        }
        body_width
    }

    pub(super) fn fields(&mut self, c: &Card, status: &PluginStatus, x: f32, mut y: f32, width: f32) {
        let label_width = 82.0;
        for field in &status.fields {
            let extra = field_extra_height(field, field.value.as_deref().unwrap_or_default());
            let top = y;
            y += FIELD_HEIGHT + extra;
            let label = c.fit(&field.label, size::SMALL, W::Regular, label_width - 6.0);
            c.centered_text(
                x,
                top + 12.0,
                &label,
                size::SMALL,
                W::Regular,
                theme::GRAY,
                H::Left,
            );
            let id = egui::Id::new((
                "plugin-field",
                self.plugin,
                self.revision,
                &field.key,
                c.live,
            ));
            let rect = c.rect(x + label_width, top, width - label_width, 24.0 + extra);
            let mut ui = egui::Ui::new(
                c.ui.ctx().clone(),
                id,
                egui::UiBuilder::new()
                    .max_rect(rect)
                    .layer_id(c.painter.layer_id())
                    .style(c.ui.style().clone()),
            );
            ui.set_clip_rect(rect.intersect(c.painter.clip_rect()));
            ui.multiply_opacity(c.painter.opacity());
            let value = self.form.entry(field.key.clone()).or_default();
            let input_id = id.with("value");
            let submit = field.multiline && c.live && self.enabled
                && ui.memory(|memory| memory.has_focus(input_id))
                && ui.input_mut(|input| input.consume_key(egui::Modifiers::CTRL, egui::Key::Enter));
            let input = if field.multiline {
                egui::TextEdit::multiline(value)
            } else {
                egui::TextEdit::singleline(value)
            }
                .id(input_id)
                .font(c.font(size::BODY, W::Regular))
                .text_color(theme::WHITE)
                .desired_width(rect.width())
                .desired_rows(1)
                .char_limit(field.max)
                .interactive(c.live && self.enabled);
            let response = if field.multiline {
                egui::ScrollArea::vertical().id_salt(id.with("scroll"))
                    .max_height(rect.height()).auto_shrink([false, false])
                    .show(&mut ui, |ui| ui.add(input)).inner
            } else {
                ui.put(rect, input.vertical_align(egui::Align::Center))
            }.on_hover_text(&field.label);
            if response.changed() {
                value.retain(|ch| field.accepts_char(ch));
                if field.multiline {
                    ui.ctx().request_repaint();
                }
            }
            if submit {
                if let Some(index) = status.actions.iter().position(|action| action.open.is_none() && action.copy.is_none()) {
                    self.clicked = Some(ClickTarget::Button(index));
                }
            }
            self.response.gained_focus |= response.gained_focus();
            self.response.focused |= c.live && self.enabled && response.has_focus();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multiline_status_field_grows_until_six_rows() {
        let field: StatusField = serde_json::from_value(serde_json::json!({
            "key":"note","label":"便签","max":100,"multiline":true
        })).unwrap();
        assert!(field.accepts("第一行\n第二行"));
        assert!(!field.accepts("第一行\u{0000}"));
        assert_eq!(field_extra_height(&field, "单行"), 0.0);
        assert_eq!(field_extra_height(&field, "第一行\n第二行"), MULTILINE_ROW_HEIGHT);
        assert_eq!(field_extra_height(&field, &"空行\n".repeat(10)), 5.0 * MULTILINE_ROW_HEIGHT);
        let legacy: StatusField = serde_json::from_value(serde_json::json!({
            "key":"note","label":"便签","max":100
        })).unwrap();
        assert!(!legacy.accepts("旧版\n单行"));
    }
}
