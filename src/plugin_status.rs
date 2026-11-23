//! 插件详情页的数据与布局契约。

use serde::{Deserialize, Serialize};

const MAX_TIMESTAMP: i64 = 253_402_300_799;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StatusHealth {
    Waiting,
    Unconfigured,
    Offline,
    Stale,
    Invalid,
    Ready,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum StatusState {
    Running,
    Paused,
    Succeeded,
    Error,
    Offline,
    Idle,
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PluginStatus {
    pub state: StatusState,
    pub label: String,
    pub summary: Option<String>,
    pub progress_pct: Option<f64>,
    #[serde(default)]
    pub metrics: Vec<StatusMetric>,
    pub alert: Option<String>,
    pub observed_at: Option<i64>,
    pub expires_at: Option<i64>,
    #[serde(default)]
    pub actions: Vec<StatusAction>,
    #[serde(default)]
    pub fields: Vec<StatusField>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StatusMetric {
    pub label: String,
    pub value: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StatusAction {
    pub id: String,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StatusField {
    pub key: String,
    pub label: String,
    pub value: Option<String>,
    pub max: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StatusLayout {
    pub components: Vec<StatusComponent>,
    #[serde(default)]
    pub style: StatusStyle,
}

impl StatusLayout {
    pub(crate) fn validate(&self) -> Result<(), &'static str> {
        if !(1..=6).contains(&self.components.len()) {
            return Err("状态页布局需要 1 至 6 个组件");
        }
        for (index, component) in self.components.iter().enumerate() {
            if self.components[..index].iter().any(|previous| {
                std::mem::discriminant(previous) == std::mem::discriminant(component)
            }) {
                return Err("状态页布局中每种组件只能出现一次");
            }
            if matches!(component, StatusComponent::Metrics { columns } if !(1..=2).contains(columns))
            {
                return Err("状态页指标只能使用 1 列或 2 列");
            }
        }
        if self.style.accent.is_some() && self.style.accent_rgb().is_none() {
            return Err("状态页强调色需为 #RRGGBB 格式");
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum StatusComponent {
    State {},
    Summary {},
    Progress {
        #[serde(default)]
        variant: ProgressVariant,
    },
    Metrics {
        #[serde(default = "default_metric_columns")]
        columns: u8,
    },
    Alert {},
    UpdatedAt {},
    Actions {},
    Fields {},
}

fn default_metric_columns() -> u8 {
    2
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ProgressVariant {
    #[default]
    Ring,
    Bar,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StatusStyle {
    #[serde(
        default,
        deserialize_with = "present_accent",
        skip_serializing_if = "Option::is_none"
    )]
    accent: Option<String>,
    #[serde(default)]
    pub density: StatusDensity,
    #[serde(default)]
    pub surface: StatusSurface,
}

fn present_accent<'de, D: serde::Deserializer<'de>>(input: D) -> Result<Option<String>, D::Error> {
    String::deserialize(input).map(Some)
}

impl StatusStyle {
    pub(crate) fn accent_rgb(&self) -> Option<[u8; 3]> {
        let hex = self.accent.as_deref()?.strip_prefix('#')?;
        if hex.len() != 6 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return None;
        }
        let [_, red, green, blue] = u32::from_str_radix(hex, 16).ok()?.to_be_bytes();
        Some([red, green, blue])
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum StatusDensity {
    #[default]
    Comfortable,
    Compact,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum StatusSurface {
    #[default]
    Card,
    Plain,
}

impl PluginStatus {
    pub(crate) fn valid(&self) -> bool {
        valid_text(&self.label, 40)
            && self
                .summary
                .as_deref()
                .is_none_or(|text| valid_text(text, 120))
            && self
                .alert
                .as_deref()
                .is_none_or(|text| valid_text(text, 120))
            && self
                .progress_pct
                .is_none_or(|pct| (0.0..=100.0).contains(&pct))
            && self.metrics.len() <= 6
            && self
                .metrics
                .iter()
                .all(|metric| valid_text(&metric.label, 28) && valid_text(&metric.value, 32))
            && self.actions.len() <= 3
            && self.actions.iter().enumerate().all(|(index, action)| {
                valid_control_id(&action.id)
                    && valid_text(&action.label, 6)
                    && self.actions[..index].iter().all(|other| other.id != action.id)
            })
            && self.fields.len() <= 3
            && self.fields.iter().enumerate().all(|(index, field)| {
                valid_control_id(&field.key)
                    && valid_text(&field.label, 28)
                    && (1..=200).contains(&field.max)
                    && field.value.as_deref().is_none_or(|value| field.accepts(value))
                    && self.fields[..index].iter().all(|other| other.key != field.key)
            })
            && self.observed_at.is_none_or(valid_timestamp)
            && self.expires_at.is_none_or(valid_timestamp)
            && self
                .observed_at
                .zip(self.expires_at)
                .is_none_or(|(at, expires)| at <= expires)
    }

    pub(crate) fn fresh_at(&self, now: i64) -> bool {
        self.expires_at.is_none_or(|expires| now < expires)
    }
}

impl StatusField {
    pub(crate) fn accepts(&self, value: &str) -> bool {
        value.chars().count() <= self.max && !value.chars().any(char::is_control)
    }
}

pub(crate) fn valid_control_id(id: &str) -> bool {
    id.len() <= 40
        && id.starts_with(|ch: char| ch.is_ascii_lowercase())
        && id.bytes().all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || matches!(ch, b'_' | b'-'))
}

pub(crate) fn valid_text(text: &str, max_chars: usize) -> bool {
    !text.trim().is_empty()
        && text.chars().count() <= max_chars
        && !text.chars().any(char::is_control)
}

fn valid_timestamp(timestamp: i64) -> bool {
    (0..=MAX_TIMESTAMP).contains(&timestamp)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn plugin_status_accepts_bounded_fields_and_honors_expiry() {
        let status: PluginStatus = serde_json::from_value(json!({
            "state":"running", "label":"打印中", "summary":"首层校准完成", "progress_pct":48.5,
            "metrics":[{"label":"喷嘴","value":"220 °C"}], "alert":"耗材余量偏低",
            "observed_at":100, "expires_at":120
        }))
        .unwrap();
        assert!(status.valid());
        assert_eq!(status.state, StatusState::Running);
        assert_eq!(status.metrics[0].value, "220 °C");
        assert!(status.fresh_at(119));
        assert!(!status.fresh_at(120));
        let idle: PluginStatus =
            serde_json::from_value(json!({"state":"idle","label":"就绪"})).unwrap();
        assert!(idle.valid() && idle.fresh_at(MAX_TIMESTAMP));
    }

    #[test]
    fn plugin_status_rejects_invalid_or_oversized_values_and_unrecognized_fields() {
        for (key, value) in [
            ("label", json!("休眠\n正在打印")),
            ("summary", json!("测".repeat(121))),
            ("alert", json!(" ")),
            ("progress_pct", json!(100.1)),
            ("observed_at", json!(i64::MAX)),
            ("expires_at", json!(99)),
            (
                "metrics",
                json!(vec![json!({"label":"温度","value":"30 °C"}); 7]),
            ),
            ("metrics", json!([{"label":"温度","value":"测".repeat(33)}])),
        ] {
            let mut input = json!({"state":"running","label":"打印中","observed_at":100});
            input[key] = value;
            let status: PluginStatus = serde_json::from_value(input).unwrap();
            assert!(!status.valid(), "{key}: {status:?}");
        }
        assert!(serde_json::from_value::<PluginStatus>(json!({
            "state":"idle","label":"就绪","unknown_field":[]
        }))
        .is_err());
        assert!(serde_json::from_value::<PluginStatus>(json!({
            "state":"unknown","label":"异常"
        }))
        .is_err());
    }
}
