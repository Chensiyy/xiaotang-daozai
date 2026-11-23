//! 脚本按钮、专注事件和提醒共用插件的串行执行队列。

use super::*;
use crate::{
    paint::Card,
    plugin_status::valid_text,
    plugin_status_ui::{self, Interaction, InteractionResponse},
};
use std::collections::{BTreeSet, VecDeque};

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum EventName {
    #[serde(rename = "focus.finished")]
    FocusFinished,
}

#[derive(Clone, Serialize)]
pub(super) struct FocusFinished {
    name: EventName,
    id: String,
    at: i64,
    minutes: u32,
    #[serde(skip)]
    sequence: u64,
}

impl FocusFinished {
    fn new(at: i64, minutes: u32) -> Self {
        static SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed) + 1;
        Self {
            name: EventName::FocusFinished,
            id: format!("{at}-{sequence}"),
            at,
            minutes,
            sequence,
        }
    }
}

pub(super) enum Call {
    Refresh,
    Action { id: String, form: Values },
    Event(FocusFinished),
}

impl Call {
    pub(super) fn prepare(self, input: &mut Value) -> &'static str {
        match self {
            Self::Refresh => "JSON.stringify(refresh(JSON.parse(__input)))",
            Self::Action { id, form } => {
                input["action"] = json!({ "id": id, "form": form });
                "JSON.stringify(action(JSON.parse(__input)))"
            }
            Self::Event(event) => {
                input["event"] = json!(event);
                "JSON.stringify(event(JSON.parse(__input)))"
            }
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ScriptNotice {
    id: String,
    title: String,
    body: Option<String>,
}

impl ScriptNotice {
    pub(super) fn valid(&self) -> bool {
        valid_text(&self.id, 128)
            && valid_text(&self.title, 40)
            && self.body.as_deref().is_none_or(|body| {
                body.chars().count() <= 120 && body.chars().all(|ch| ch == '\n' || !ch.is_control())
            })
    }
}

#[derive(Clone, Default, Deserialize, Serialize)]
pub(super) struct NoticeHistory {
    ids: BTreeSet<String>,
    last_ms: Option<i64>,
}

impl NoticeHistory {
    pub(super) fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    #[cfg(windows)]
    fn accept(&mut self, id: &str, now_ms: i64) -> bool {
        if self
            .last_ms
            .is_some_and(|last| now_ms.saturating_sub(last) < 60_000)
            || !self.ids.insert(id.to_owned())
        {
            return false;
        }
        self.last_ms = Some(now_ms);
        true
    }
}

#[derive(Default)]
pub(super) struct InteractionState {
    pub(super) revision: u64,
    pub(super) forms: BTreeMap<String, Values>,
    pub(super) notices: BTreeMap<String, ScriptNotice>,
    calls: BTreeMap<String, VecDeque<Call>>,
    last_event: u64,
}

impl InteractionState {
    pub(super) fn has_call(&self, id: &str) -> bool {
        self.calls.contains_key(id)
    }

    pub(super) fn pop_call(&mut self, id: &str) -> Call {
        let Some(queue) = self.calls.get_mut(id) else {
            return Call::Refresh;
        };
        let call = queue.pop_front().unwrap();
        if queue.is_empty() {
            self.calls.remove(id);
        }
        call
    }

    pub(super) fn forget(&mut self, id: &str) {
        self.calls.remove(id);
        self.forms.remove(id);
        self.notices.remove(id);
    }

    pub(super) fn retain(&mut self, keep: impl Fn(&str) -> bool) {
        self.calls.retain(|id, _| keep(id));
        self.forms.retain(|id, _| keep(id));
        self.notices.retain(|id, _| keep(id));
    }
}

impl Manifest {
    pub(super) fn has_capability(&self, name: &str) -> bool {
        self.capabilities
            .iter()
            .any(|capability| capability == name)
    }
}

impl Record {
    pub(super) fn owner_on(&self, tasks_on: bool, calendar_on: bool, quota_on: bool) -> bool {
        let Some(package) = &self.package else {
            return false;
        };
        match package.manifest.owner.as_str() {
            "tasks" => tasks_on && !package.manifest.is_retired_lunar_preview(),
            "calendar" => calendar_on,
            "quota" => quota_on,
            "status" => {
                !self.status_page_disabled
                    && (self.source_kind().is_none() || self.source_grant().is_some())
            }
            _ => false,
        }
    }
}

impl LocalPlugins {
    pub(crate) fn focus_finished(
        &mut self,
        at: i64,
        minutes: u32,
        tasks_on: bool,
        calendar_on: bool,
        quota_on: bool,
    ) {
        self.queue_event(
            FocusFinished::new(at, minutes),
            tasks_on,
            calendar_on,
            quota_on,
        );
    }

    fn queue_event(
        &mut self,
        event: FocusFinished,
        tasks_on: bool,
        calendar_on: bool,
        quota_on: bool,
    ) {
        if event.sequence <= self.interactions.last_event {
            return;
        }
        self.interactions.last_event = event.sequence;
        for (id, record) in &self.records {
            if record.enabled
                && record.owner_on(tasks_on, calendar_on, quota_on)
                && record
                    .package
                    .as_ref()
                    .and_then(|package| package.manifest.events.as_ref())
                    .is_some_and(|events| events.contains(&event.name))
            {
                self.interactions
                    .calls
                    .entry(id.clone())
                    .or_default()
                    .push_back(Call::Event(event.clone()));
            }
        }
    }

    pub(crate) fn status_height(&self, id: &str) -> f32 {
        let layout = self.status_layout(id);
        let default_controls = layout.is_none()
            && self
                .records
                .get(id)
                .and_then(|record| record.package.as_ref())
                .is_some_and(|package| package.manifest.has_capability("action"));
        plugin_status_ui::status_page_height(layout)
            + if default_controls {
                plugin_status_ui::DEFAULT_FORM_HEIGHT
            } else {
                0.0
            }
    }

    pub(crate) fn draw_status_controls(&mut self, c: &Card, id: &str) -> InteractionResponse {
        let Some((_, status, health)) = self.status_page(id) else {
            return InteractionResponse::default();
        };
        let layout = self.status_layout(id);
        let Some(controls) =
            status.filter(|status| !status.actions.is_empty() || !status.fields.is_empty())
        else {
            plugin_status_ui::draw_status_page(c, status, health, layout);
            return InteractionResponse::default();
        };
        let status = controls.clone();
        let layout = layout.cloned();
        let revision = self.status[id].as_ref().unwrap().0;
        let enabled = self.action_available(id);
        let form = self
            .interactions
            .forms
            .entry(id.to_owned())
            .or_insert_with(|| {
                status
                    .fields
                    .iter()
                    .map(|field| (field.key.clone(), field.value.clone().unwrap_or_default()))
                    .collect()
            });
        let mut interaction = Interaction::new(id, revision, form, enabled);
        plugin_status_ui::draw_interactive_status_page(
            c,
            &status,
            layout.as_ref(),
            &mut interaction,
        );
        let (response, action) = interaction.finish();
        if let Some(action) = action {
            self.submit_action(id, revision, action);
        }
        response
    }

    fn action_available(&self, id: &str) -> bool {
        !self.jobs.contains_key(id) && !self.interactions.has_call(id)
    }

    fn submit_action(&mut self, id: &str, revision: u64, action: String) -> bool {
        let Some((_, Some(status), StatusHealth::Ready)) = self.status_page(id) else {
            return false;
        };
        if !self.action_available(id)
            || !matches!(self.status.get(id), Some(Ok((current, _))) if *current == revision)
            || !status.actions.iter().any(|button| button.id == action)
        {
            return false;
        }
        let values = self.interactions.forms.get(id);
        let form = status
            .fields
            .iter()
            .map(|field| {
                let value = values
                    .and_then(|values| values.get(&field.key))
                    .or(field.value.as_ref())
                    .cloned()
                    .unwrap_or_default();
                (field.key.clone(), value)
            })
            .collect();
        self.interactions
            .calls
            .entry(id.to_owned())
            .or_default()
            .push_back(Call::Action { id: action, form });
        true
    }

    #[cfg(windows)]
    pub(crate) fn take_script_notices(
        &mut self,
        enabled: bool,
        now_ms: i64,
    ) -> Vec<crate::plugin_notice::Event> {
        use crate::plugin_notice::{Completion, Event};
        let pending = std::mem::take(&mut self.interactions.notices);
        if !enabled {
            return Vec::new();
        }
        pending
            .into_iter()
            .filter_map(|(id, notice)| {
                let mut record = self.records.get(&id)?.clone();
                if !record.notice_history.accept(&notice.id, now_ms) {
                    return None;
                }
                let name = record.package.as_ref()?.manifest.name.clone();
                let data = serde_json::to_vec(&record).ok()?;
                // 提醒回执和私有数据共用可重启读取的原子快照。
                if data.len() > 2 * 1024 * 1024
                    || crate::model::atomic_write(&self.path(&id), &data).is_err()
                {
                    return None;
                }
                self.records.insert(id.clone(), record);
                Some(Event::Completed(Completion {
                    source: format!("sdk:{id}"),
                    name,
                    id: notice.id,
                    session: None,
                    markdown: notice.body.unwrap_or_default(),
                    title: Some(notice.title),
                    action_label: None,
                }))
            })
            .collect()
    }
}

#[cfg(test)]
#[path = "plugin_interactions_tests.rs"]
mod tests;
