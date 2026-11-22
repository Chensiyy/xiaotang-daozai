use super::*;
use crate::plugin_status::StatusHealth;
use serde_json::{json, Value};
use std::{
    fs,
    path::Path,
    time::{Duration, Instant},
};

fn install(h: &mut H, profile: &Path, id: &str, name: &str, data: &Value) {
    let folder = profile.join(id);
    fs::create_dir_all(&folder).unwrap();
    fs::write(
        folder.join("cover.png"),
        include_bytes!("../spike/examples/payday/cover.png"),
    )
    .unwrap();
    fs::write(
        folder.join("main.js"),
        r#"
function refresh({source}) {
    return {status: source.status === "ready" ? JSON.parse(source.text) : null};
}
"#,
    )
    .unwrap();
    let source = folder.join("status.json");
    fs::write(&source, data.to_string()).unwrap();
    let manifest = folder.join("plugin.json");
    fs::write(
        &manifest,
        json!({
            "id":id, "name":name, "author":"验收", "version":"1.0.0", "api":1,
            "runtime":"js", "entry":"main.js", "cover":"cover.png", "owner":"status",
            "capabilities":["status.page","timer"], "dataVersion":1,
            "permissions":{"source.read":"file"}, "settings":[]
        })
        .to_string(),
    )
    .unwrap();
    h.app.plugins.install(&manifest).unwrap();
    let installed = profile
        .join("local-plugins")
        .join(format!("local-{id}.json"));
    let mut record: Value = serde_json::from_slice(&fs::read(&installed).unwrap()).unwrap();
    record["source_grant"] = json!({"File":source});
    fs::write(installed, record.to_string()).unwrap();
}

fn has_text(h: &H, label: &str) -> bool {
    h.shapes
        .iter()
        .any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.text() == label))
}

fn settled(h: &mut H, id: &str, expected: StatusHealth) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        h.frame(vec![]);
        if h.app
            .plugins
            .status_page(id)
            .is_some_and(|(_, _, health)| health == expected)
        {
            return;
        }
        assert!(Instant::now() < deadline, "状态未就绪：{id} / {expected:?}");
        std::thread::sleep(Duration::from_millis(4));
    }
}

#[test]
fn plugin_status_pages_render_from_authorized_data_and_keep_the_native_carousel() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let mut h = H::new();
    let profile = crate::model::config_dir();
    let printer = "example.a-printer";
    let companion = "example.b-companion";
    let todo = crate::app::TODO_PAGE;
    h.app.settings.visible_pages.fill(false);
    h.app.settings.visible_pages[todo] = true;
    install(
        &mut h,
        &profile,
        printer,
        "拓竹打印（模拟）",
        &json!({
            "state":"running", "label":"打印中", "summary":"拓竹模拟任务 · 收纳盒",
            "progress_pct":48,
            "metrics":[
                {"label":"喷嘴温度","value":"220 °C"}, {"label":"热床温度","value":"60 °C"},
                {"label":"层数","value":"58 / 120"}, {"label":"剩余时间","value":"24 分钟"}
            ],
            "alert":"耗材余量偏低", "observed_at":1793785200
        }),
    );
    install(
        &mut h,
        &profile,
        companion,
        "模拟状态源",
        &json!({
            "state":"idle", "label":"另一来源就绪", "metrics":[{"label":"已处理","value":"42 项"}]
        }),
    );
    h.app.plugins = crate::sdk_spike::local::LocalPlugins::open(profile.join("local-plugins"));
    h.app.plugins.enable(printer, true).unwrap();
    h.app.plugins.enable(companion, true).unwrap();
    h.app.show_page(todo);
    let cursor = H::local(225.0, 80.0);
    h.hover(cursor, 0.75);
    settled(&mut h, printer, StatusHealth::Ready);
    settled(&mut h, companion, StatusHealth::Ready);
    h.wheel(-40.0);
    h.hover(cursor, 0.65);
    assert_eq!(h.app.page(), crate::app::PAGE_NAMES.len());
    assert!(has_text(&h, "拓竹打印（模拟）"));
    assert!(has_text(&h, "打印中"));
    assert!(has_text(&h, "48"));
    assert!(has_text(&h, "220 °C"));
    assert!(has_text(&h, "耗材余量偏低"));
    if let Some(path) = std::env::var_os("DAOZAI_PLUGIN_STATUS_PREVIEW") {
        let path = std::path::PathBuf::from(path);
        fs::create_dir_all(&path).unwrap();
        shoot_feature(
            &mut h,
            &mut feature_raster(),
            &path,
            "拓竹结构化状态页",
            30,
            Some(cursor),
            "2026-11-04",
        );
    }

    h.wheel(-40.0);
    h.hover(cursor, 0.65);
    assert!(has_text(&h, "另一来源就绪"));
    h.app
        .plugins
        .set_status_page_enabled(printer, false)
        .unwrap();
    h.frame(vec![]);
    assert_eq!(h.app.page(), crate::app::PAGE_NAMES.len());
    assert!(
        has_text(&h, "另一来源就绪"),
        "其他插件页的编号移动后应保留所选来源"
    );
    assert!(h.app.plugins.status_page(printer).is_none());

    fs::remove_file(profile.join(printer).join("status.json")).unwrap();
    h.app
        .plugins
        .set_status_page_enabled(printer, true)
        .unwrap();
    settled(&mut h, printer, StatusHealth::Offline);
    h.app.show_status_page(printer);
    h.hover(cursor, 0.65);
    assert!(has_text(&h, "数据源失联"));
    assert!(!has_text(&h, "220 °C"));

    h.app
        .plugins
        .set_status_page_enabled(printer, false)
        .unwrap();
    h.frame(vec![]);
    assert_eq!(h.app.page(), todo, "关闭当前插件页后应回到仍开启的内置页");
    assert!(has_text(&h, "待办"));
    assert!(h
        .app
        .plugins
        .status_page(companion)
        .is_some_and(|(_, _, health)| health == StatusHealth::Ready));
}
