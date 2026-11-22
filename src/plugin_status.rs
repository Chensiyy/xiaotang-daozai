//! 插件详情页的只读数据契约。

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
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StatusMetric {
    pub label: String,
    pub value: String,
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

fn valid_text(text: &str, max_chars: usize) -> bool {
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
            "state":"idle","label":"就绪","actions":[]
        }))
        .is_err());
        assert!(serde_json::from_value::<PluginStatus>(json!({
            "state":"unknown","label":"异常"
        }))
        .is_err());
    }
}
