use super::*;
use crate::plugin_status::StatusComponent;
use eframe::egui;

const ID: &str = "example.water-log";
const TODAY: &str = "2026-11-23";
const MANIFEST: &str = include_str!("examples/water-log/plugin.json");

struct Fixture {
    root: PathBuf,
    manifest: PathBuf,
}

impl Fixture {
    fn new() -> Result<Self> {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("daozai-actions-{}-{stamp}", std::process::id()));
        fs::create_dir(&root)?;
        let manifest = root.join("plugin.json");
        fs::write(&manifest, MANIFEST)?;
        fs::write(
            root.join("cover.png"),
            include_bytes!("examples/water-log/cover.png"),
        )?;
        fs::write(
            root.join("main.js"),
            include_str!("examples/water-log/main.js"),
        )?;
        Ok(Self { root, manifest })
    }

    fn host(&self) -> Result<LocalPlugins> {
        let mut host = LocalPlugins::open(self.root.join("installed"));
        host.install(&self.manifest)?;
        host.enable(ID, true)?;
        settle(&mut host);
        Ok(host)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn settle(host: &mut LocalPlugins) {
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        host.tick(true, true, TODAY);
        if host.jobs.is_empty() {
            break;
        }
        assert!(Instant::now() < until, "等待插件刷新超时");
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn revision(host: &LocalPlugins) -> u64 {
    host.status[ID].as_ref().unwrap().0
}

fn shown(host: &LocalPlugins) -> &str {
    &host.status_page(ID).unwrap().1.unwrap().label
}

#[test]
fn invalid_manifest_combinations_and_interaction_fields_are_rejected() -> Result<()> {
    let manifest = Manifest::decode(MANIFEST)?;
    assert!(manifest
        .status_layout
        .unwrap()
        .components
        .contains(&StatusComponent::Actions {}));
    let base: Value = serde_json::from_str(MANIFEST)?;
    for (field, value) in [
        (
            "capabilities",
            json!(["action", "event", "notice", "storage"]),
        ),
        (
            "capabilities",
            json!(["status.page", "event", "notice", "storage"]),
        ),
        (
            "capabilities",
            json!(["status.page", "action", "notice", "storage"]),
        ),
        ("events", json!(["unknown"])),
        ("events", json!([])),
        ("events", json!(["focus.finished", "focus.finished"])),
    ] {
        let mut invalid = base.clone();
        invalid[field] = value;
        assert!(Manifest::decode(&invalid.to_string()).is_err(), "{invalid}");
    }
    assert!(capabilities::check("js", "tasks", &["card".into(), "action".into()]).is_err());
    assert!(capabilities::check(
        "js",
        "tasks",
        &["card".into(), "status.page".into(), "action".into()],
    )
    .is_ok());

    for (field, value) in [
        ("actions", json!([{"id":"add","label":"一二三四五六七"}])),
        ("actions", json!(vec![json!({"id":"add","label":"加"}); 4])),
        (
            "actions",
            json!([{"id":"add","label":"加"},{"id":"add","label":"增加"}]),
        ),
        ("fields", json!([{"key":"note","label":"备注","max":201}])),
        (
            "fields",
            json!([{"key":"note","label":"备注","max":3,"value":"换行\n"}]),
        ),
        (
            "fields",
            json!([{"key":"note","label":"备注","max":1,"value":"两字"}]),
        ),
        (
            "fields",
            json!(vec![json!({"key":"note","label":"备注","max":3}); 4]),
        ),
    ] {
        let mut output = json!({"state":"idle","label":"就绪"});
        output[field] = value;
        assert!(!serde_json::from_value::<PluginStatus>(output)?.valid());
    }
    Ok(())
}

#[test]
fn stale_buttons_and_late_results_are_discarded() -> Result<()> {
    let fixture = Fixture::new()?;
    let mut host = fixture.host()?;
    let old = revision(&host);
    assert!(host.submit_action(ID, old, "add".into()));
    assert!(!host.submit_action(ID, old, "add".into()));
    settle(&mut host);
    assert_eq!(shown(&host), "1 / 8 杯");
    assert!(!host.submit_action(ID, old, "add".into()));

    let current = revision(&host);
    assert!(host.submit_action(ID, current, "add".into()));
    host.tick(true, true, TODAY);
    assert!(host.jobs.contains_key(ID));
    host.enable(ID, false)?;
    settle(&mut host);
    assert!(!host.submit_action(ID, current, "add".into()));
    assert_eq!(host.records[ID].storage["cups"], 1);
    host.enable(ID, true)?;
    settle(&mut host);
    assert!(!host.submit_action(ID, current, "add".into()));

    let current = revision(&host);
    assert!(host.submit_action(ID, current, "add".into()));
    host.tick(true, true, TODAY);
    host.save(ID, Values::new())?;
    settle(&mut host);
    assert_eq!(host.records[ID].storage["cups"], 1);
    assert!(!host.submit_action(ID, current, "add".into()));
    Ok(())
}

#[test]
fn duplicate_focus_event_is_discarded_and_disabled_plugins_do_not_replay_it() -> Result<()> {
    let fixture = Fixture::new()?;
    let mut host = fixture.host()?;
    let finished = FocusFinished::new(1_700_000_000, 25);
    host.queue_event(finished.clone(), true, true, false);
    host.queue_event(finished.clone(), true, true, false);
    settle(&mut host);
    host.queue_event(finished, true, true, false);
    settle(&mut host);
    assert_eq!(shown(&host), "1 / 8 杯");

    host.set_status_page_enabled(ID, false)?;
    let paused = FocusFinished::new(1_700_001_500, 25);
    host.queue_event(paused.clone(), true, true, false);
    host.set_status_page_enabled(ID, true)?;
    host.queue_event(paused, true, true, false);
    settle(&mut host);
    assert_eq!(shown(&host), "1 / 8 杯");

    let current = revision(&host);
    assert!(host.submit_action(ID, current, "add".into()));
    host.tick(true, true, TODAY);
    host.queue_event(FocusFinished::new(1_700_003_000, 25), true, true, false);
    settle(&mut host);
    assert_eq!(shown(&host), "3 / 8 杯");
    Ok(())
}

#[cfg(windows)]
#[test]
fn notices_respect_disable_deduplication_cooldown_and_restart() -> Result<()> {
    use crate::plugin_notice::Event;
    let fixture = Fixture::new()?;
    let mut host = fixture.host()?;
    let give = |host: &mut LocalPlugins, id: &str| -> Result<()> {
        host.interactions.notices.insert(
            ID.into(),
            serde_json::from_value(json!({
                "id": id, "title": "喝水目标完成", "body": "已记满 8 杯水",
            }))?,
        );
        Ok(())
    };
    give(&mut host, "today")?;
    assert!(host.take_script_notices(false, 0).is_empty());
    for _ in 0..8 {
        assert!(host.submit_action(ID, revision(&host), "add".into()));
        settle(&mut host);
    }
    let first = host.take_script_notices(true, 0);
    assert!(
        matches!(&first[..], [Event::Completed(notice)] if notice.title.as_deref() == Some("喝水目标完成"))
    );
    give(&mut host, "next")?;
    assert!(host.take_script_notices(true, 59_999).is_empty());
    give(&mut host, &format!("water-{TODAY}"))?;
    assert!(host.take_script_notices(true, 60_000).is_empty());
    give(&mut host, "next")?;
    assert_eq!(host.take_script_notices(true, 60_000).len(), 1);

    let mut reopened = LocalPlugins::open(fixture.root.join("installed"));
    give(&mut reopened, "next")?;
    assert!(reopened.take_script_notices(true, 120_000).is_empty());
    Ok(())
}

#[test]
fn status_page_egui_button_submits_form_and_updates_output() -> Result<()> {
    use super::super::management_test::{click, click_at, frame, has_text};
    use crate::paint::Card;

    let fixture = Fixture::new()?;
    let mut host = fixture.host()?;
    let current = revision(&host);
    let ctx = egui::Context::default();
    crate::theme::load_fonts(&ctx);
    let mut draw = |ui: &mut egui::Ui| {
        let card = Card::new(ui, egui::Pos2::ZERO, ui.max_rect());
        host.draw_status_controls(&card, ID);
    };
    let before = frame(&ctx, vec![], &mut draw);
    assert!(has_text(&before, "0 / 8 杯"));
    let note_id = egui::Id::new(("plugin-field", ID, current, "note", true)).with("value");
    let pos = ctx.read_response(note_id).unwrap().rect.center();
    click_at(&ctx, pos, &mut draw);
    assert!(ctx.memory(|memory| memory.has_focus(note_id)));
    frame(&ctx, vec![egui::Event::Text("常温".into())], &mut draw);
    click(&ctx, "+1 杯", draw);
    settle(&mut host);

    let after = frame(&ctx, vec![], |ui| {
        let card = Card::new(ui, egui::Pos2::ZERO, ui.max_rect());
        host.draw_status_controls(&card, ID);
    });
    assert!(has_text(&after, "1 / 8 杯"));
    assert_eq!(host.records[ID].storage["note"], "常温");
    let record: Record = serde_json::from_str(&fs::read_to_string(host.path(ID))?)?;
    assert_eq!(record.storage["note"], "常温");
    assert_eq!(record.storage["cups"], 1);
    Ok(())
}
