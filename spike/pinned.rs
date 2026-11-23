use super::{fail, fs, valid_id, LocalPlugins, Path, Result, Surface};
use crate::plugin_status::{StatusCountdown, StatusCounter};
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

const MAX_PINNED: usize = 3;
const ROTATION: Duration = Duration::from_secs(8);
const MASKED_VALUE: &str = "••••";
const ORDER_FILE: &str = "island-pinned.json";

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Pinned {
    text: String,
    value: Option<PinnedValue>,
    icon: Option<PinnedIcon>,
    #[serde(default)]
    sensitive: bool,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PinnedIcon {
    Clock,
    Chart,
    Coin,
    Calendar,
    Cup,
    Cloud,
}

#[derive(Clone, Deserialize)]
#[serde(untagged)]
enum PinnedValue {
    Text(String),
    Number(f64),
    Live(LiveValue),
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
enum LiveValue {
    Counter(StatusCounter),
    Countdown(StatusCountdown),
}

fn valid_text(text: &str, max: usize) -> bool {
    !text.trim().is_empty() && text.chars().count() <= max && !text.chars().any(char::is_control)
}

impl Pinned {
    pub(super) fn valid(&self) -> bool {
        valid_text(&self.text, 12)
            && self.value.as_ref().is_none_or(|value| match value {
                PinnedValue::Text(text) => valid_text(text, 24),
                PinnedValue::Number(number) => number.is_finite(),
                PinnedValue::Live(LiveValue::Counter(counter)) => counter.valid(),
                PinnedValue::Live(LiveValue::Countdown(countdown)) => countdown.valid(),
            })
    }

    fn display(&self, now: chrono::DateTime<chrono::Utc>) -> (PinnedDisplay, Option<Duration>) {
        let second = now.timestamp();
        let hide = self.sensitive
            || matches!(&self.value,
            Some(PinnedValue::Live(LiveValue::Counter(counter))) if counter.sensitive);
        let (text, value, next) = if hide {
            if self.value.is_some() {
                (self.text.clone(), Some(MASKED_VALUE.into()), None)
            } else {
                (MASKED_VALUE.into(), None, None)
            }
        } else {
            let (value, next) = match &self.value {
                None => (None, None),
                Some(PinnedValue::Text(text)) => (Some(text.clone()), None),
                Some(PinnedValue::Number(number)) => (Some(number.to_string()), None),
                Some(PinnedValue::Live(LiveValue::Counter(counter))) => {
                    (Some(counter.value_at(second)), counter.repaint_at(second))
                }
                Some(PinnedValue::Live(LiveValue::Countdown(countdown))) => (
                    Some(countdown.value_at(second)),
                    countdown.repaint_at(second),
                ),
            };
            (self.text.clone(), value, next)
        };
        let display = PinnedDisplay {
            text,
            value,
            icon: self.icon,
        };
        (
            display,
            next.map(|deadline| {
                Duration::from_millis((deadline * 1000 - now.timestamp_millis()).max(1) as u64)
            }),
        )
    }
}

pub(crate) struct PinnedDisplay {
    pub text: String,
    pub value: Option<String>,
    pub icon: Option<PinnedIcon>,
}

impl PinnedDisplay {
    #[cfg(test)]
    pub(crate) fn caption(&self) -> String {
        self.value.as_ref().map_or_else(
            || self.text.clone(),
            |value| format!("{}  {value}", self.text),
        )
    }

    pub(crate) fn draw(
        &self,
        card: &crate::paint::Card,
        x: f32,
        y: f32,
        width: f32,
        vertical: bool,
    ) {
        use crate::{
            app,
            paint::{self, H},
            theme::{self, W},
        };
        let icon = self.icon.map(|icon| match icon {
            PinnedIcon::Clock => app::FOCUS_PAGE,
            PinnedIcon::Chart => app::PERFORMANCE_PAGE,
            PinnedIcon::Coin => app::QUOTA_PAGE,
            PinnedIcon::Calendar => app::CALENDAR_PAGE,
            PinnedIcon::Cup => app::WATER_PAGE,
            PinnedIcon::Cloud => app::WEATHER_PAGE,
        });
        if vertical {
            if let Some(page) = icon {
                paint::page_shortcut_icon(card, page, x, y - 13.0, theme::WHITE);
            }
            let title = card.fit(&self.text, 9.0, W::Regular, width - 6.0);
            let split = self.value.is_some() || icon.is_some();
            card.centered_text(
                x,
                y - if split { 1.0 } else { 0.0 },
                &title,
                9.0,
                W::Regular,
                theme::WHITE,
                H::Center,
            );
            if let Some(value) = &self.value {
                let value = card.fit(value, 9.0, W::Semibold, width - 6.0);
                card.centered_text(
                    x,
                    y + 11.0,
                    &value,
                    9.0,
                    W::Semibold,
                    theme::WHITE,
                    H::Center,
                );
            }
        } else {
            let size = 11.0;
            let inset = if icon.is_some() { 16.0 } else { 0.0 };
            let room = width - inset - 4.0;
            let label = if let Some(value) = &self.value {
                let value = card.fit(value, size, W::Semibold, room * 0.66);
                let label = card.fit(
                    &self.text,
                    size,
                    W::Semibold,
                    room - card.text_w(&value, size, W::Semibold) - 6.0,
                );
                format!("{label}  {value}")
            } else {
                card.fit(&self.text, size, W::Semibold, room)
            };
            let left = x - (card.text_w(&label, size, W::Semibold) + inset) / 2.0;
            if let Some(page) = icon {
                paint::page_shortcut_icon(card, page, left + 5.0, y, theme::WHITE);
            }
            card.centered_text(
                left + inset,
                y,
                &label,
                size,
                W::Semibold,
                theme::WHITE,
                H::Left,
            );
        }
    }
}

#[derive(Default)]
pub(super) struct PinnedState {
    pub(super) order: Vec<String>,
    pub(super) output: BTreeMap<String, Pinned>,
    pub(super) replacement: Option<String>,
    active: Vec<String>,
    started: Option<Instant>,
}

impl PinnedState {
    pub(super) fn open(dir: &Path) -> Self {
        let order = super::bounded(&dir.join(ORDER_FILE), 1024)
            .ok()
            .and_then(|text| serde_json::from_str::<Vec<String>>(&text).ok())
            .unwrap_or_default();
        let mut clean = Vec::new();
        for id in order.into_iter().filter(|id| valid_id(id)) {
            if !clean.contains(&id) && clean.len() < MAX_PINNED {
                clean.push(id);
            }
        }
        Self {
            order: clean,
            ..Self::default()
        }
    }

    fn save(&mut self, dir: &Path, order: Vec<String>) -> Result<()> {
        crate::model::atomic_write(&dir.join(ORDER_FILE), &serde_json::to_vec(&order)?)?;
        self.order = order;
        Ok(())
    }

    fn display(
        &mut self,
        suppressed: bool,
        instant: Instant,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Option<(PinnedDisplay, Option<Duration>)> {
        if suppressed {
            return None;
        }
        let active = self
            .order
            .iter()
            .filter(|id| self.output.contains_key(*id))
            .cloned()
            .collect::<Vec<_>>();
        if active != self.active {
            self.active = active;
            self.started = Some(instant);
        }
        if self.active.is_empty() {
            return None;
        }
        let elapsed = instant.saturating_duration_since(*self.started.get_or_insert(instant));
        let index = (elapsed.as_secs() / ROTATION.as_secs()) as usize % self.active.len();
        let (display, update) = self.output[&self.active[index]].display(now);
        let rotate = (self.active.len() > 1).then(|| {
            ROTATION - Duration::from_millis((elapsed.as_millis() % ROTATION.as_millis()) as u64)
        });
        let next = update.into_iter().chain(rotate).min();
        Some((display, next))
    }
}

impl LocalPlugins {
    pub(crate) fn pinned_display(
        &mut self,
        suppressed: bool,
    ) -> Option<(PinnedDisplay, Option<Duration>)> {
        self.pinned
            .display(suppressed, Instant::now(), chrono::Utc::now())
    }

    pub(super) fn pinned_name(&self, id: &str) -> Option<&str> {
        let package = self.records.get(id)?.package.as_ref()?;
        package
            .manifest
            .has_surface(Surface::IslandPinned)
            .then_some(package.manifest.name.as_str())
    }

    pub(super) fn set_pinned(&mut self, id: &str, enabled: bool) -> Result<()> {
        let mut order = self.pinned.order.clone();
        if enabled {
            if self.pinned_name(id).is_none() {
                return Err(fail("此插件未声明岛上常驻位"));
            }
            if order.iter().any(|item| item == id) {
                return Ok(());
            }
            if order.len() == MAX_PINNED {
                self.pinned.replacement = Some(id.to_owned());
                return Ok(());
            }
            order.push(id.to_owned());
        } else {
            order.retain(|item| item != id);
            if order == self.pinned.order {
                return Ok(());
            }
        }
        self.pinned.save(&self.dir, order)
    }

    pub(super) fn replace_pinned(&mut self, old: &str) -> Result<()> {
        let Some(id) = self.pinned.replacement.as_deref() else {
            return Ok(());
        };
        if self.pinned_name(id).is_none() {
            return Err(fail("待添加的插件未安装或未声明常驻位"));
        }
        let mut order = self.pinned.order.clone();
        let at = order
            .iter()
            .position(|item| item == old)
            .ok_or_else(|| fail("该常驻位已更改"))?;
        order[at] = id.to_owned();
        self.pinned.save(&self.dir, order)?;
        self.pinned.replacement = None;
        Ok(())
    }

    pub(super) fn move_pinned(&mut self, from: usize, to: usize) -> Result<()> {
        let mut order = self.pinned.order.clone();
        order.swap(from, to);
        self.pinned.save(&self.dir, order)
    }

    pub(super) fn restore_pins(&mut self) {
        let mut order = self.pinned.order.clone();
        order.retain(|id| self.pinned_name(id).is_some());
        if order != self.pinned.order {
            if let Err(error) = self.pinned.save(&self.dir, order.clone()) {
                self.notice = error.to_string();
            }
            self.pinned.order = order;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{App, Env, Options, TODO_PAGE, WIN_H, WIN_W};
    use eframe::egui;
    use serde_json::json;

    struct Fixture(std::path::PathBuf);

    impl Fixture {
        fn new() -> Result<Self> {
            let path = std::env::temp_dir().join(format!(
                "daozai-pinned-{}-{}",
                std::process::id(),
                chrono::Utc::now().timestamp_nanos_opt().unwrap()
            ));
            fs::create_dir_all(&path)?;
            Ok(Self(path))
        }

        fn install(&self, plugins: &mut LocalPlugins, id: &str) -> Result<()> {
            let package = self.0.join(id);
            fs::create_dir(&package)?;
            let sample =
                Path::new(env!("CARGO_MANIFEST_DIR")).join("spike/examples/payday/cover.png");
            fs::copy(sample, package.join("cover.png"))?;
            let manifest = package.join("plugin.json");
            fs::write(&manifest, json!({
                "id":id,"name":id,"author":"岛仔","version":"1.0.0","api":1,"runtime":"js",
                "entry":"main.js","cover":"cover.png","owner":"tasks","capabilities":["card","island.pinned"],
                "dataVersion":1,"permissions":{},"settings":[],
            }).to_string())?;
            fs::write(
                package.join("main.js"),
                "function refresh() { return {pinned:{text:'常驻示例',value:'25',icon:'clock'}}; }",
            )?;
            plugins.install(&manifest)?;
            plugins.enable(id, true)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    struct TestConfig(Option<std::ffi::OsString>);

    impl TestConfig {
        fn use_dir(path: &Path) -> Self {
            let old = std::env::var_os("DAOZAI_DATA_DIR");
            std::env::set_var("DAOZAI_DATA_DIR", path);
            Self(old)
        }
    }

    impl Drop for TestConfig {
        fn drop(&mut self) {
            if let Some(old) = &self.0 {
                std::env::set_var("DAOZAI_DATA_DIR", old);
            } else {
                std::env::remove_var("DAOZAI_DATA_DIR");
            }
        }
    }

    #[test]
    fn pinned_rotates_yields_and_masks_value() {
        let mut state = PinnedState::default();
        for id in ["one", "two", "three"] {
            state.order.push(id.into());
            state.output.insert(
                id.into(),
                serde_json::from_value(serde_json::json!({
                    "text":id,"value":"10000","sensitive":true,
                }))
                .unwrap(),
            );
        }
        let t = Instant::now();
        let now = chrono::Utc::now();
        assert_eq!(
            state.display(false, t, now).unwrap().0.caption(),
            "one  ••••"
        );
        assert_eq!(
            state.display(false, t + ROTATION, now).unwrap().0.text,
            "two"
        );
        assert!(state.display(true, t + ROTATION, now).is_none());
        state.output.remove("one");
        assert_eq!(
            state.display(false, t + ROTATION, now).unwrap().0.text,
            "two"
        );
    }

    #[test]
    fn pinned_counter_and_countdown_follow_second_boundaries() -> Result<()> {
        let now = chrono::DateTime::from_timestamp_millis(100_250).unwrap();
        let counter =
            json!({"start_value":100,"per_second":2,"start_at":100,"until":102,"decimals":0});
        let pinned: Pinned =
            serde_json::from_value(json!({"text":"工资","value":{"counter":counter}}))?;
        assert_eq!(pinned.display(now).0.value.as_deref(), Some("100"));
        assert_eq!(pinned.display(now).1, Some(Duration::from_millis(750)));
        let done = now + chrono::TimeDelta::seconds(2);
        assert_eq!(pinned.display(done).0.value.as_deref(), Some("104"));
        assert!(pinned.display(done).1.is_none());
        let mut counter = counter;
        counter["sensitive"] = json!(true);
        let hidden: Pinned =
            serde_json::from_value(json!({"text":"工资","value":{"counter":counter}}))?;
        assert_eq!(hidden.display(now).0.value.as_deref(), Some(MASKED_VALUE));
        assert!(hidden.display(now).1.is_none());
        let countdown: Pinned =
            serde_json::from_value(json!({"text":"下班","value":{"countdown":{
                "label":"距离下班","target_at":102,"format":"HH:mm:ss","done_label":"已下班"
            }}}))?;
        assert_eq!(countdown.display(now).0.value.as_deref(), Some("00:00:02"));
        assert_eq!(countdown.display(now).1, Some(Duration::from_millis(750)));
        assert_eq!(countdown.display(done).0.value.as_deref(), Some("已下班"));
        assert!(countdown.display(done).1.is_none());
        Ok(())
    }

    #[test]
    fn pinned_preference_is_bounded_and_requires_explicit_replacement() -> Result<()> {
        let fixture = Fixture::new()?;
        let dir = fixture.0.join("records");
        let mut plugins = LocalPlugins::open(dir.clone());
        for id in ["pinned.one", "pinned.two", "pinned.three", "pinned.four"] {
            fixture.install(&mut plugins, id)?;
            plugins.set_pinned(id, true)?;
        }
        assert_eq!(
            plugins.pinned.order,
            ["pinned.one", "pinned.two", "pinned.three"]
        );
        assert_eq!(plugins.pinned.replacement.as_deref(), Some("pinned.four"));
        plugins.replace_pinned("pinned.two")?;
        plugins.move_pinned(1, 0)?;
        let mut plugins = LocalPlugins::open(dir);
        assert_eq!(
            plugins.pinned.order,
            ["pinned.four", "pinned.one", "pinned.three"]
        );
        plugins.uninstall("pinned.four", false)?;
        assert_eq!(plugins.pinned.order, ["pinned.one", "pinned.three"]);
        plugins.pinned.output.insert(
            "pinned.one".into(),
            serde_json::from_value(json!({"text":"第一个插件"}))?,
        );
        assert!(plugins.pinned_display(false).is_some());
        plugins.enable("pinned.one", false)?;
        assert!(plugins.pinned_display(false).is_none());
        assert_eq!(plugins.pinned.order, ["pinned.one", "pinned.three"]);
        Ok(())
    }

    #[test]
    fn pinned_setting_adds_a_capsule_and_hides_on_owner_disable() -> Result<()> {
        let _lock = crate::ui_tests::ENV_LOCK
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        let fixture = Fixture::new()?;
        let _config = TestConfig::use_dir(&fixture.0.join("config"));
        let ctx = egui::Context::default();
        let mut app = App::new(
            &ctx,
            Options {
                fetch_lyrics: false,
                ..Default::default()
            },
        );
        app.settings.minimal = false;
        app.settings.browser_minimal = false;
        app.settings.pet = false;
        app.settings.dock = crate::model::Dock::Top;
        app.settings.skin = crate::model::Skin::Classic;
        app.settings.visible_pages[TODO_PAGE] = true;
        app.plugins = LocalPlugins::open(fixture.0.join("records"));
        let id = "pinned.example";
        fixture.install(&mut app.plugins, id)?;
        let settings_rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(450.0, 350.0));
        let mut settings = |events| {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(settings_rect),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default()
                        .show(ctx, |ui| app.plugins.settings(ui, "tasks", true));
                },
            );
        };
        settings(vec![]);
        let pos = ctx
            .read_response(egui::Id::new((
                "settings-plugin-switch",
                "pinned.example:pinned",
            )))
            .unwrap()
            .rect
            .center();
        for pressed in [true, false] {
            settings(vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ]);
        }
        assert_eq!(app.plugins.pinned.order, [id]);
        let win = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(WIN_W, WIN_H));
        let frame = |app: &mut App, t| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(win),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        app.frame(
                            ui,
                            win,
                            &Env {
                                t,
                                clock: "12:30:00".into(),
                                today: "2026-11-23".into(),
                                ..Default::default()
                            },
                        );
                    });
                },
            )
        };
        let has_pinned = |output: egui::FullOutput| {
            output.shapes.iter().any(|shape| {
            matches!(&shape.shape, egui::Shape::Text(text) if text.galley.text().contains("常驻示例"))
        })
        };
        let mut shown = false;
        for i in 1..60 {
            if has_pinned(frame(&mut app, i as f64 / 60.0)) {
                shown = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(shown, "收起的岛显示常驻内容");
        app.notice = Some(crate::extras::Alert {
            source: crate::extras::AlertSource::Internal,
            title: "测试弹窗".into(),
            body: String::new(),
            page: TODO_PAGE,
            at: 1.0,
            duration: 4.0,
            bluetooth: None,
            meeting: None,
            unread: None,
        });
        assert!(!has_pinned(frame(&mut app, 1.1)), "通知弹窗优先显示");
        assert!(app.notice.is_some());
        app.notice = None;
        let restored = (1..90).any(|index| has_pinned(frame(&mut app, 1.1 + index as f64 / 60.0)));
        assert!(restored, "弹窗退场后恢复常驻");
        app.settings.visible_pages[TODO_PAGE] = false;
        assert!(!has_pinned(frame(&mut app, 3.0)));
        Ok(())
    }
}
