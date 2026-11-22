use super::*;
use crate::quota::{relay, Account, Provider};
use serde_json::json;
use std::{
    fs,
    time::{Duration, Instant},
};

fn install(app: &mut App, id: &str, name: &str, script: &str, source: Option<&std::path::Path>) {
    let folder = crate::model::config_dir().join(id);
    fs::create_dir_all(&folder).unwrap();
    image::RgbaImage::new(16, 16)
        .save(folder.join("cover.png"))
        .unwrap();
    fs::write(folder.join("main.js"), script).unwrap();
    let manifest = folder.join("plugin.json");
    let permissions = if source.is_some() {
        json!({"source.read":"file"})
    } else {
        json!({})
    };
    fs::write(&manifest, json!({
        "id":id, "name":name, "author":"验收", "version":"1.0.0", "api":1,
        "runtime":"js", "entry":"main.js", "cover":"cover.png", "owner":"quota",
        "capabilities":["quota.source","timer"], "dataVersion":1, "permissions":permissions, "settings":[]
    }).to_string()).unwrap();
    app.quota_plugins.install(&manifest).unwrap();
    if let Some(source) = source {
        app.quota_plugins.grant_file(id, source).unwrap();
    }
    app.quota_plugins.enable(id, true).unwrap();
}

fn shows(h: &H, label: &str) -> bool {
    h.shapes
        .iter()
        .any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.text() == label))
}

fn wait_for_sources(h: &mut H, count: usize) {
    let deadline = Instant::now() + Duration::from_secs(4);
    while h.app.quota_plugins.quota_sources().count() != count {
        h.app.quota_plugins.tick(true);
        assert!(Instant::now() < deadline, "额度来源未更新");
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn plugin_quota_uses_the_existing_panel_without_changing_builtin_accounts() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|err| err.into_inner());
    let mut h = H::new();
    let baseline = "native-account";
    h.app.quota.config.accounts.push(Account {
        id: baseline.into(),
        alias: "内置来源".into(),
        provider: Provider::Sub2Api,
        source: "https://example.invalid".into(),
        slot: None,
        enabled: true,
        pinned: false,
    });
    h.app.quota.config.selected = baseline.into();
    h.app.quota.rows.insert(
        baseline.into(),
        Ok(relay::parse_sub2api(&json!({
            "mode":"unrestricted","isValid":true,"unit":"USD","balance":23
        }))
        .unwrap()),
    );
    let original = h.app.quota.config.clone();
    let source = crate::model::config_dir().join("mock-quota.json");
    fs::create_dir_all(source.parent().unwrap()).unwrap();
    fs::write(&source, r#"{"remaining":680,"limit":1000}"#).unwrap();
    install(
        &mut h.app,
        "example.panel-first",
        "模拟积分源",
        r#"function refresh({source}) {
            if (source.status !== 'ready') return {quota:null};
            const {remaining, limit} = JSON.parse(source.text);
            return {quota:{windows:[{label:'当月模拟积分',remaining,limit,unit:'积分'}]}};
        }"#,
        Some(&source),
    );
    install(
        &mut h.app,
        "example.panel-second",
        "模拟周期源",
        "function refresh() { return {quota:{windows:[{label:'本周模拟额度',remaining_pct:42}]}}; }",
        None,
    );
    h.app.settings.visible_pages[crate::app::QUOTA_PAGE] = true;
    h.app.show_page(crate::app::QUOTA_PAGE);
    wait_for_sources(&mut h, 2);
    h.hover(H::local(230.0, 65.0), 0.8);
    assert!(shows(&h, "内置来源"));
    assert!(shows(&h, "模拟积分源"));
    h.click_text("模拟积分源");
    h.hover(H::local(230.0, 65.0), 0.25);
    assert!(shows(&h, "当月模拟积分"));
    assert!(shows(&h, "680 / 1000 积分"));
    assert_eq!(h.app.quota.config, original);
    assert_eq!(h.app.quota.rows.len(), 1);

    h.app
        .quota_plugins
        .enable("example.panel-first", false)
        .unwrap();
    h.frame(vec![]);
    assert!(!shows(&h, "模拟积分源"));
    assert!(shows(&h, "23 USD"));
    assert!(shows(&h, "模拟周期源"));
    h.click_text("模拟周期源");
    h.hover(H::local(230.0, 65.0), 0.25);
    assert!(shows(&h, "本周模拟额度"));
    assert!(shows(&h, "42%"));
    h.app
        .quota_plugins
        .enable("example.panel-first", true)
        .unwrap();
    wait_for_sources(&mut h, 2);
    h.frame(vec![]);
    h.click_text("模拟积分源");
    h.hover(H::local(230.0, 65.0), 0.25);
    assert!(shows(&h, "680 / 1000 积分"));
    fs::remove_file(source).unwrap();
    h.app.quota_plugins.refresh();
    wait_for_sources(&mut h, 1);
    h.frame(vec![]);
    assert!(!shows(&h, "模拟积分源"));
    assert!(shows(&h, "模拟周期源"));
    assert!(shows(&h, "23 USD"));
    assert_eq!(h.app.quota.config, original);
}
