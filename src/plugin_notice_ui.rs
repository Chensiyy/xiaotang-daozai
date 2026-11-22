use crate::app::{App, BAR_H, Cmd, Env};
use crate::extras::{Alert, AlertSource};
use crate::paint::{self, Card, H};
use crate::plugin_notice::{Completion, Event};
use crate::theme::{self, W, size};
use eframe::egui::{self, Color32};
use std::collections::VecDeque;

pub(crate) const DETAIL_HEIGHT: f32 = 204.0;
const MAX_UNREAD: usize = 32;
const COLLAPSED_BADGES: usize = 3;
const RED_DOT: Color32 = Color32::from_rgb(255, 59, 48);

#[derive(Default)]
pub(crate) struct NoticeUi {
    pub(crate) unread: VecDeque<Completion>,
    detail: Option<Completion>,
    opening: Vec<(String, String)>,
}

impl NoticeUi {
    fn sources(&self) -> Vec<&Completion> {
        let mut sources: Vec<&Completion> = Vec::new();
        for notice in &self.unread {
            if !sources.iter().any(|current| current.source == notice.source) {
                sources.push(notice);
            }
        }
        sources
    }

    pub(crate) fn collapsed_count(&self) -> usize {
        self.sources().len().min(COLLAPSED_BADGES)
    }

    fn hide_content(&mut self) {
        for notice in self.unread.iter_mut().chain(self.detail.iter_mut()) {
            notice.markdown = String::new();
        }
    }

    fn find(&self, source: &str, id: &str) -> Option<&Completion> {
        self.unread.iter().chain(self.detail.iter()).find(|notice| {
            notice.source == source && notice.id == id
        })
    }
}

impl App {
    pub(crate) fn receive_plugin_notice(&mut self, event: Event) {
        if !self.settings.notifications {
            return;
        }
        match event {
            Event::Completed(mut completion) => {
                if self.plugin_notice_ui.find(&completion.source, &completion.id).is_some() {
                    return;
                }
                if !self.settings.notification_preview {
                    completion.markdown = String::new();
                }
                let alert = Alert {
                    title: completion.name.clone(),
                    source: AlertSource::Plugin { source: completion.source.clone(), id: completion.id.clone() },
                    body: "本轮已结束".into(),
                    page: usize::MAX,
                    at: self.t,
                    duration: 4.5,
                    bluetooth: None,
                    meeting: None,
                    unread: None,
                };
                self.plugin_notice_ui.unread.push_front(completion);
                if self.plugin_notice_ui.unread.len() > MAX_UNREAD {
                    if let Some(removed) = self.plugin_notice_ui.unread.pop_back() {
                        self.clear_plugin_notice_alerts(Some(&removed.source), Some(&removed.id));
                    }
                }
                self.enqueue_alert(alert, false);
            }
            Event::Opened { source, id, ok } => {
                let previous = self.plugin_notice_ui.opening.len();
                self.plugin_notice_ui.opening.retain(|(s, i)| *s != source || *i != id);
                if self.plugin_notice_ui.opening.len() == previous {
                    return;
                }
                if ok {
                    self.mark_plugin_notice_read(&source, &id);
                    if self.plugin_notice_ui.detail.as_ref().is_some_and(|notice| notice.source == source && notice.id == id) {
                        self.plugin_notice_ui.detail = None;
                    }
                } else {
                    self.messenger_unavailable("暂时无法打开会话");
                }
            }
        }
    }

    pub(crate) fn remove_plugin_notice_source(&mut self, source: &str) {
        self.plugin_notice_ui.unread.retain(|notice| notice.source != source);
        self.plugin_notice_ui.opening.retain(|(id, _)| id != source);
        if self.plugin_notice_ui.detail.as_ref().is_some_and(|notice| notice.source == source) {
            self.plugin_notice_ui.detail = None;
        }
        self.clear_plugin_notice_alerts(Some(source), None);
    }

    fn mark_plugin_notice_read(&mut self, source: &str, id: &str) {
        self.plugin_notice_ui.unread.retain(|notice| notice.source != source || notice.id != id);
        self.clear_plugin_notice_alerts(Some(source), Some(id));
    }

    fn clear_plugin_notice_alerts(&mut self, source: Option<&str>, id: Option<&str>) {
        let matches_notice = |alert: &Alert| match &alert.source {
            AlertSource::Plugin { source: s, id: i } => source.is_none_or(|want| want == s) && id.is_none_or(|want| want == i),
            _ => false,
        };
        self.alerts.retain(|alert| !matches_notice(alert));
        if self.notice.as_ref().is_some_and(matches_notice) {
            self.notice = None;
        }
        if self.hud_notice.as_ref().is_some_and(matches_notice) {
            self.hud_notice = None;
        }
    }

    pub(crate) fn plugin_notice_detail_open(&self) -> bool {
        self.plugin_notice_ui.detail.is_some()
    }

    pub(crate) fn tick_plugin_notices(&mut self, ctx: &egui::Context, env: &Env) {
        if !self.settings.notifications {
            self.plugin_notice_ui = NoticeUi::default();
            self.clear_plugin_notice_alerts(None, None);
            return;
        }
        if !self.settings.notification_preview || env.screen_locked {
            self.plugin_notice_ui.hide_content();
        }
        if self.plugin_notice_detail_open() && (env.screen_locked || env.fullscreen_hidden
            || ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape))) {
            self.plugin_notice_ui.detail = None;
        }
    }

    fn show_plugin_notice_detail(&mut self, source: &str, id: &str) {
        if let Some(notice) = self.plugin_notice_ui.find(source, id).cloned() {
            self.mark_plugin_notice_read(source, id);
            self.plugin_notice_ui.detail = Some(notice);
            self.reveal_unread();
        }
    }

    fn open_plugin_notice(&mut self, source: &str, id: &str, cmds: &mut Vec<Cmd>) {
        if self.plugin_notice_ui.opening.iter().any(|(s, i)| s == source && i == id) {
            return;
        }
        let session = self.plugin_notice_ui.find(source, id).and_then(|notice| notice.session.clone());
        if let Some(session) = session.filter(|value| !value.is_empty()) {
            self.plugin_notice_ui.opening.push((source.into(), id.into()));
            cmds.push(Cmd::OpenPluginNotice { source: source.into(), id: id.into(), session });
        } else {
            self.show_plugin_notice_detail(source, id);
        }
    }

    pub(crate) fn draw_plugin_notice_banner(&mut self, c: &Card, alert: &Alert, source: &str, id: &str, cmds: &mut Vec<Cmd>) {
        let Some(notice) = self.plugin_notice_ui.find(source, id) else { return; };
        source_icon(c, 22.0, 22.0, &alert.title);
        c.centered_text(42.0, 14.0, &c.fit(&alert.title, 11.0, W::Semibold, 84.0), 11.0, W::Semibold, theme::WHITE, H::Left);
        c.centered_text(42.0, 30.0, &alert.body, 10.5, W::Regular, theme::GRAY, H::Left);
        let preview = self.settings.notification_preview && !notice.markdown.trim().is_empty();
        let session = notice.session.is_some();
        let open = session && button(c, 204.0, 10.0, 64.0, "回会话", ("notice-banner-open", source, id));
        let details = preview && button(c, if session { 132.0 } else { 204.0 }, 10.0, 66.0, "查看回复", ("notice-banner-body", source, id));
        let close = button(c, 274.0, 10.0, 47.0, "关闭", ("notice-banner-close", source, id));
        if close {
            self.notice = None;
        } else if details {
            self.show_plugin_notice_detail(source, id);
        } else if open {
            self.open_plugin_notice(source, id, cmds);
        }
    }

    pub(crate) fn draw_plugin_notice_capsule(&mut self, c: &Card, offset: usize, slot: f32, height: f32, scroll: f32, width: f32, cmds: &mut Vec<Cmd>) {
        let mut clicked = None;
        for (index, notice) in self.plugin_notice_ui.unread.iter().enumerate() {
            let x = width / 2.0;
            let y = 2.0 + slot * ((offset + index) as f32 + 0.5) - scroll;
            if y < -slot || y > height + slot { continue; }
            source_icon(c, x, y, &notice.name);
            c.circle(x + 10.0, y - 9.0, 2.6, RED_DOT);
            if c.hit(0.0, y - slot / 2.0, width, slot, ("plugin-capsule", &notice.source, &notice.id))
                .on_hover_text(format!("查看{}的完成提醒", notice.name)).clicked() {
                clicked = Some((notice.source.clone(), notice.id.clone()));
            }
        }
        if let Some((source, id)) = clicked {
            self.open_plugin_notice(&source, &id, cmds);
        }
    }

    pub(crate) fn draw_plugin_notice_stack(&mut self, c: &Card, horizontal: bool, offset: usize, step: f32, length: f32, cmds: &mut Vec<Cmd>) {
        let sources = self.plugin_notice_ui.sources();
        let count = sources.len().min(COLLAPSED_BADGES);
        let mut expand = false;
        let mut clicked = None;
        for (i, notice) in sources.iter().take(count).enumerate() {
            let index = offset + i;
            let (x, y) = if horizontal { (16.0 + step * index as f32, 14.0) } else { (16.0, 14.0 + step * index as f32) };
            let multiple_sources = i + 1 == count && sources.len() > count;
            if multiple_sources {
                c.centered_text(x, y, "+", 14.0, W::Semibold, theme::WHITE, H::Center);
            } else {
                source_icon(c, x, y, &notice.name);
            }
            c.circle(x + 8.0, y - 7.0, 2.6, RED_DOT);
            let center = if horizontal { x } else { y };
            let start = if index == 0 { 0.0 } else { center - step / 2.0 };
            let end = if i + 1 == count { length } else { center + step / 2.0 };
            let id = ("plugin-stack", &notice.source);
            let hit = if horizontal { c.hit(start, 0.0, end - start, BAR_H, id) } else { c.hit(0.0, start, 32.0, end - start, id) };
            let multiple_notices = self.plugin_notice_ui.unread.iter().filter(|item| item.source == notice.source).take(2).count() > 1;
            let label = if multiple_sources || multiple_notices { "展开查看完成提醒".into() } else { format!("查看{}的完成提醒", notice.name) };
            if hit.on_hover_text(label).clicked() {
                if multiple_sources || multiple_notices {
                    expand = true;
                } else {
                    clicked = Some((notice.source.clone(), notice.id.clone()));
                }
            }
        }
        if expand {
            self.reveal_unread();
        } else if let Some((source, id)) = clicked {
            self.open_plugin_notice(&source, &id, cmds);
        }
    }

    pub(crate) fn draw_plugin_notice_detail(&mut self, c: &Card, cmds: &mut Vec<Cmd>) {
        let Some(notice) = self.plugin_notice_ui.detail.as_ref() else { return; };
        source_icon(c, 24.0, 27.0, &notice.name);
        let heading = format!("{} · 本轮已结束", notice.name);
        c.centered_text(43.0, 27.0, &c.fit(&heading, size::TITLE, W::Semibold, 220.0), size::TITLE, W::Semibold, theme::WHITE, H::Left);
        let close = button(c, 364.0, 15.0, 42.0, "关闭", "plugin-detail-close");
        let open = notice.session.is_some() && button(c, 287.0, 15.0, 70.0, "回会话", "plugin-detail-open");
        c.fill(15.0, 46.0, 389.0, 144.0, 9.0, theme::CARD);
        let rect = c.rect(27.0, 55.0, 365.0, 127.0);
        let body = if !self.settings.notification_preview {
            "通知正文已隐藏"
        } else if notice.markdown.trim().is_empty() {
            "没有可预览的回复正文"
        } else {
            &notice.markdown
        };
        let scroll_id = egui::Id::new(("plugin-notice-markdown", &notice.source, &notice.id));
        let mut ui = egui::Ui::new(c.ui.ctx().clone(), scroll_id, egui::UiBuilder::new()
            .max_rect(rect).layer_id(c.painter.layer_id()).style(c.ui.style().clone()));
        ui.set_clip_rect(rect.intersect(c.painter.clip_rect()));
        egui::ScrollArea::vertical().auto_shrink([false, false]).max_height(rect.height()).show(&mut ui, |ui| {
            draw_markdown(ui, body);
        });
        if close {
            self.plugin_notice_ui.detail = None;
        } else if open {
            let (source, id) = (notice.source.clone(), notice.id.clone());
            self.open_plugin_notice(&source, &id, cmds);
        }
    }
}

fn button(c: &Card, x: f32, y: f32, width: f32, text: &str, id: impl std::hash::Hash) -> bool {
    let hit = c.hit(x, y, width, 24.0, id);
    let (hover, press) = c.hover_t(&hit);
    let inner = c.scaled(x + width / 2.0, y + 12.0, 1.0 - 0.05 * press);
    inner.fill(x, y, width, 24.0, 12.0, paint::mix(theme::BTN, theme::BTN_HOVER, hover));
    inner.centered_text(x + width / 2.0, y + 12.0, text, size::SMALL, W::Semibold, theme::WHITE, H::Center);
    hit.clicked()
}

fn source_icon(c: &Card, x: f32, y: f32, name: &str) {
    c.circle(x, y, 9.0, theme::BTN);
    let letter = name.chars().find(|ch| ch.is_alphanumeric()).unwrap_or('·');
    c.centered_text(x, y, &letter.to_string(), 10.5, W::Bold, theme::WHITE, H::Center);
}

fn draw_markdown(ui: &mut egui::Ui, text: &str) {
    let mut code = false;
    for line in text.lines() {
        let line = line.trim_end();
        if line.trim_start().starts_with("```") {
            code = !code;
            continue;
        }
        if line.is_empty() {
            ui.add_space(4.0);
            continue;
        }
        let line = if code {
            egui::RichText::new(line).monospace().size(size::BODY)
        } else if let Some((prefix, title)) = line.split_once(' ').filter(|(prefix, _)| {
            (1..=6).contains(&prefix.len()) && prefix.bytes().all(|byte| byte == b'#')
        }) {
            let font = if prefix.len() < 3 { size::TITLE } else { size::BODY };
            egui::RichText::new(title).font(theme::font(font, W::Semibold))
        } else if let Some(item) = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")) {
            egui::RichText::new(format!("•  {item}")).size(size::BODY)
        } else {
            egui::RichText::new(line).size(size::BODY)
        };
        ui.add(egui::Label::new(line.color(theme::WHITE)).wrap().selectable(false));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn completion(id: &str) -> Event {
        Event::Completed(Completion {
            source: "example.agent".into(),
            name: "示例助手".into(),
            id: id.into(),
            session: Some("session-1".into()),
            markdown: "# 私有回复".into(),
        })
    }

    #[test]
    fn plugin_notice_remains_unread_until_open_is_confirmed() {
        let _guard = crate::ui_tests::ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
        let ctx = egui::Context::default();
        let mut app = App::new(&ctx, Default::default());
        app.settings.notifications = true;
        app.receive_plugin_notice(completion("one"));
        app.receive_plugin_notice(completion("two"));
        app.tick_alerts();
        app.t += 5.0;
        app.tick_alerts();
        assert!(!matches!(&app.notice, Some(alert) if matches!(&alert.source, AlertSource::Plugin { id, .. } if id == "one")));
        assert_eq!(app.plugin_notice_ui.unread.len(), 2);

        let mut cmds = Vec::new();
        app.open_plugin_notice("example.agent", "one", &mut cmds);
        app.open_plugin_notice("example.agent", "one", &mut cmds);
        assert!(matches!(cmds.as_slice(), [Cmd::OpenPluginNotice { id, session, .. }] if id == "one" && session == "session-1"));
        app.receive_plugin_notice(Event::Opened { source: "example.agent".into(), id: "one".into(), ok: false });
        assert_eq!(app.plugin_notice_ui.unread.len(), 2);

        app.open_plugin_notice("example.agent", "one", &mut cmds);
        app.receive_plugin_notice(Event::Opened { source: "example.agent".into(), id: "one".into(), ok: true });
        assert_eq!(app.plugin_notice_ui.unread.len(), 1);
        assert_eq!(app.plugin_notice_ui.unread[0].id, "two");
    }

    #[test]
    fn plugin_notice_drops_private_content_when_preview_is_disabled() {
        let _guard = crate::ui_tests::ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
        let ctx = egui::Context::default();
        let mut app = App::new(&ctx, Default::default());
        app.settings.notifications = true;
        app.settings.notification_preview = true;
        app.receive_plugin_notice(completion("one"));
        app.show_plugin_notice_detail("example.agent", "one");
        assert!(app.plugin_notice_ui.unread.is_empty());
        assert_eq!(app.plugin_notice_ui.detail.as_ref().unwrap().markdown, "# 私有回复");

        app.settings.notification_preview = false;
        app.tick_plugin_notices(&ctx, &Env::default());
        app.receive_plugin_notice(completion("two"));
        assert!(app.plugin_notice_ui.detail.as_ref().unwrap().markdown.is_empty());
        assert!(app.plugin_notice_ui.unread[0].markdown.is_empty());
        assert!(app.history.iter().all(|alert| !alert.body.contains("私有回复")));
        app.remove_plugin_notice_source("example.agent");
        assert!(app.plugin_notice_ui.unread.is_empty());
        assert!(!app.plugin_notice_detail_open());
    }
}
