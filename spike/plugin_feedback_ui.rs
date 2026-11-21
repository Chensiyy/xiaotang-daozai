//! 插件详情和已安装列表共用的反馈入口。
use eframe::egui;

const PRIVATE_REPORT: &str =
    "https://github.com/Renly1994/xiaotang-daozai-plugins/security/advisories/new";

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Topic {
    #[default]
    Plugin,
    Install,
    Private,
}

impl Topic {
    fn title(self) -> &'static str {
        match self {
            Self::Plugin => "问题或建议",
            Self::Install => "安装与更新",
            Self::Private => "安全或隐私",
        }
    }
}

#[derive(Clone)]
struct Feedback {
    id: String,
    name: String,
    version: String,
    topic: Topic,
    copied: bool,
}

impl Feedback {
    fn title(&self) -> String {
        format!("[{}] {}", self.topic.title(), self.name)
    }

    fn draft(&self) -> String {
        format!(
            "插件：{}\n插件 ID：{}\n插件版本：{}\n岛仔版本：{}\n\n## {}\n\n## 复现步骤\n",
            self.name,
            self.id,
            self.version,
            env!("CARGO_PKG_VERSION"),
            self.topic.title(),
        )
    }

    fn form_url(&self) -> url::Url {
        if self.topic == Topic::Private {
            return url::Url::parse(PRIVATE_REPORT).expect("私密报告地址有效");
        }
        let mut url =
            url::Url::parse("https://github.com/Renly1994/xiaotang-daozai-plugins/issues/new")
                .expect("插件反馈地址有效");
        url.query_pairs_mut()
            .append_pair("title", &self.title())
            .append_pair("body", &self.draft());
        url
    }
}

pub(super) fn request_plugin_feedback(ui: &egui::Ui, id: &str, name: &str, version: &str) {
    let feedback = Feedback {
        id: id.to_owned(),
        name: name.split_whitespace().collect::<Vec<_>>().join(" "),
        version: version.to_owned(),
        topic: Topic::Plugin,
        copied: false,
    };
    ui.data_mut(|data| data.insert_temp(egui::Id::new("plugin-feedback"), feedback));
    ui.ctx().request_repaint();
}

pub(super) fn plugin_feedback(ui: &egui::Ui) {
    let id = egui::Id::new("plugin-feedback");
    let Some(mut feedback) = ui.data(|data| data.get_temp::<Feedback>(id)) else {
        return;
    };
    let modal = egui::Modal::new(id.with("dialog")).show(ui.ctx(), |ui| {
        ui.set_width(330.0);
        ui.heading("插件反馈");
        ui.label(format!("{} · v{}", feedback.name, feedback.version));
        ui.add_space(6.0);
        let before = feedback.topic;
        ui.horizontal_wrapped(|ui| {
            for topic in [Topic::Plugin, Topic::Install, Topic::Private] {
                ui.selectable_value(&mut feedback.topic, topic, topic.title());
            }
        });
        if feedback.topic != before {
            feedback.copied = false;
        }
        ui.add_space(4.0);
        let private = feedback.topic == Topic::Private;
        let help = match feedback.topic {
            Topic::Plugin => {
                "在 GitHub 补充使用场景或复现步骤，提交后会通知插件维护者。问题单公开可见。"
            }
            Topic::Install => "在 GitHub 补充遇到的错误和操作步骤，由岛仔维护者跟进。",
            Topic::Private => "在 GitHub 私密报告中粘贴插件信息，补充情况后提交给维护者。",
        };
        ui.label(egui::RichText::new(help).weak());
        ui.add_space(7.0);
        let mut close = false;
        ui.horizontal_wrapped(|ui| {
            let open = if private {
                "复制并打开私密报告"
            } else {
                "打开 GitHub 草稿"
            };
            if ui.button(open).clicked() {
                if private {
                    ui.ctx().copy_text(feedback.draft());
                    feedback.copied = true;
                }
                ui.ctx()
                    .open_url(egui::OpenUrl::new_tab(feedback.form_url().to_string()));
            }
            if ui.button("复制反馈信息").clicked() {
                ui.ctx().copy_text(feedback.draft());
                feedback.copied = true;
            }
            close = ui.button("关闭").clicked();
        });
        if feedback.copied {
            ui.label(egui::RichText::new("已复制反馈信息").weak());
        }
        close
    });
    let close = modal.inner || modal.should_close();
    ui.data_mut(|data| {
        if close {
            data.remove::<Feedback>(id);
        } else {
            data.insert_temp(id, feedback);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn issue_draft_preserves_plugin_context_in_query_values() {
        let feedback = Feedback {
            id: "example.payday".into(),
            name: "发薪 & 旅行".into(),
            version: "1.0.1".into(),
            topic: Topic::Install,
            copied: false,
        };
        let url = feedback.form_url();
        assert_eq!(url.host_str(), Some("github.com"));
        assert!(url.path().ends_with("/xiaotang-daozai-plugins/issues/new"));
        let query = url.query_pairs().collect::<BTreeMap<_, _>>();
        assert_eq!(query.get("title").unwrap(), "[安装与更新] 发薪 & 旅行");
        assert_eq!(query.get("body").unwrap().as_ref(), feedback.draft());
        assert!(feedback.draft().contains("插件 ID：example.payday"));
    }

    #[test]
    fn private_feedback_keeps_draft_out_of_url() {
        let feedback = Feedback {
            id: "example.payday".into(),
            name: "发薪倒计时".into(),
            version: "1.0.1".into(),
            topic: Topic::Private,
            copied: false,
        };
        let form = feedback.form_url();
        assert_eq!(form.host_str(), Some("github.com"));
        assert!(form.path().ends_with("/security/advisories/new"));
        assert!(form.query().is_none());
        assert!(feedback
            .draft()
            .starts_with("插件：发薪倒计时\n插件 ID：example.payday\n插件版本：1.0.1\n"));
        assert!(feedback.draft().contains("## 安全或隐私"));
    }
}
