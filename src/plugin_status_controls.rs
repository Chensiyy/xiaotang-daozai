use std::collections::BTreeMap;

use eframe::egui;

use crate::{
    paint::{self, Card, H},
    plugin_status::PluginStatus,
    theme::{self, size, W},
};

pub(super) const MAX_FIELDS: usize = 3;
pub(super) const ACTION_HEIGHT: f32 = 26.0;
const FIELD_HEIGHT: f32 = 31.0;
pub(crate) const DEFAULT_FORM_HEIGHT: f32 = FIELD_HEIGHT * MAX_FIELDS as f32 + ACTION_HEIGHT;

pub(super) fn fields_height(count: usize) -> f32 {
    FIELD_HEIGHT * count as f32
}

#[derive(Default)]
pub(crate) struct InteractionResponse {
    pub(crate) focused: bool,
    pub(crate) gained_focus: bool,
}

pub(crate) struct Interaction<'a> {
    plugin: &'a str,
    revision: u64,
    form: &'a mut BTreeMap<String, String>,
    enabled: bool,
    clicked: Option<String>,
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

    pub(crate) fn finish(self) -> (InteractionResponse, Option<String>) {
        (self.response, self.clicked)
    }

    pub(super) fn actions(&mut self, c: &Card, status: &PluginStatus, x: f32, y: f32, width: f32) {
        let gap = 6.0;
        let width = ((width + gap) / status.actions.len().max(1) as f32 - gap).min(98.0);
        for (index, action) in status.actions.iter().enumerate() {
            let x = x + index as f32 * (width + gap);
            let card = if self.enabled {
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
                self.clicked = Some(action.id.clone());
            }
        }
    }

    pub(super) fn fields(&mut self, c: &Card, status: &PluginStatus, x: f32, y: f32, width: f32) {
        let label_width = 82.0;
        for (index, field) in status.fields.iter().enumerate() {
            let top = y + index as f32 * FIELD_HEIGHT;
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
            let rect = c.rect(x + label_width, top, width - label_width, 24.0);
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
            let input = egui::TextEdit::singleline(value)
                .id(id.with("value"))
                .font(c.font(size::BODY, W::Regular))
                .vertical_align(egui::Align::Center)
                .text_color(theme::WHITE)
                .desired_width(rect.width())
                .desired_rows(1)
                .char_limit(field.max)
                .interactive(c.live && self.enabled);
            let response = ui.put(rect, input).on_hover_text(&field.label);
            if response.changed() {
                value.retain(|ch| !ch.is_control());
            }
            self.response.gained_focus |= response.gained_focus();
            self.response.focused |= c.live && self.enabled && response.has_focus();
        }
    }
}
