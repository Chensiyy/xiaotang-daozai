const PAYROLL_SAMPLE_AT: i64 = 1_795_500_300;

fn payroll_display(start_value: f64, per_second: f64) -> PinnedDisplay {
    let pinned: Pinned = serde_json::from_value(json!({
        "canvas": {
            "width": 110.0, "fit_width": true, "height": 22.0,
            "binds": {"cash": {"counter": {
                "start_value": start_value, "per_second": per_second,
                "start_at": PAYROLL_SAMPLE_AT, "decimals": 2,
            }}},
            "nodes": [
                {"circle": [10, 11, 7.5], "fill": "#3DDC84"},
                {"text": "¥", "at": [10, 15], "size": 10.5,
                    "fill": "#112118", "align": "center", "weight": 800},
                {"text": "{cash}", "at": [25, 16], "size": 12,
                    "fill": "$text", "max_width": 83, "roll": true, "weight": 800},
            ]
        }
    }))
    .unwrap();
    let mut display = pinned
        .display(chrono::DateTime::from_timestamp(PAYROLL_SAMPLE_AT, 0).unwrap())
        .0;
    display.plugin = "daozai.live-pay".into();
    display
}

fn payroll_frame(
    ctx: &egui::Context,
    display: &PinnedDisplay,
    slot: egui::Rect,
    vertical: bool,
    time: f64,
) -> (egui::FullOutput, Option<Duration>, bool) {
    let bounds = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(144.0, 54.0));
    let theme = CanvasTheme {
        palette: None,
        accent: None,
        background: egui::Color32::BLACK,
    };
    let now = chrono::DateTime::from_timestamp_millis(
        PAYROLL_SAMPLE_AT * 1000 + (time * 1000.0).round() as i64,
    )
    .unwrap();
    let mut repaint_after = None;
    let mut animating = false;
    let frame = ctx.run(
        egui::RawInput {
            screen_rect: Some(bounds),
            time: Some(time),
            predicted_dt: 0.0,
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show(ctx, |ui| {
                    let card = crate::paint::Card::new(ui, egui::Pos2::ZERO, bounds);
                    let (clicked, next, rolling) = display.draw_at(
                        &card,
                        slot.center().x,
                        slot.center().y,
                        slot.width(),
                        vertical,
                        theme,
                        now,
                    );
                    assert!(!clicked);
                    repaint_after = next;
                    animating = rolling;
                });
        },
    );
    (frame, repaint_after, animating)
}

fn payroll_numbers(
    frame: &egui::FullOutput,
) -> impl Iterator<Item = (&egui::epaint::TextShape, egui::Rect)> {
    frame.shapes.iter().filter_map(|shape| match &shape.shape {
        egui::Shape::Text(text)
            if !text.galley.text().is_empty()
                && text
                    .galley
                    .text()
                    .chars()
                    .all(|ch| ch.is_ascii_digit() || ch == '.') =>
        {
            Some((text, shape.clip_rect))
        }
        _ => None,
    })
}

fn capture_payroll_pinned(
    ctx: &egui::Context,
    name: &str,
    first: &egui::FullOutput,
    frame: &egui::FullOutput,
) {
    let Some(path) = std::env::var_os("DAOZAI_CANVAS_SCREENSHOTS") else {
        return;
    };
    let path = std::path::PathBuf::from(path);
    std::fs::create_dir_all(&path).unwrap();
    let mut raster = crate::preview_raster::Raster::new(144, 54);
    raster.update_textures(&first.textures_delta);
    raster.update_textures(&frame.textures_delta);
    raster.buf.fill([0.015, 0.015, 0.020, 1.0]);
    let primitives = ctx.tessellate(frame.shapes.clone(), frame.pixels_per_point);
    raster.draw(&primitives, frame.pixels_per_point);
    let mut rgba = Vec::new();
    raster.to_rgba8(&mut rgba);
    image::RgbaImage::from_raw(144, 54, rgba)
        .unwrap()
        .save(path.join(name))
        .unwrap();
}

#[test]
fn live_pay_horizontal_pinned_digits_have_tiles_and_keep_rolling() {
    let display = payroll_display(98.98, 1.01);
    let ctx = egui::Context::default();
    crate::theme::load_fonts(&ctx);
    let slot = egui::Rect::from_center_size(egui::pos2(72.0, 27.0), egui::vec2(110.0, 22.0));
    let (warm, _, _) = payroll_frame(&ctx, &display, slot, false, 0.0);
    let (first, next, animating) = payroll_frame(&ctx, &display, slot, false, 0.02);
    assert!(!animating);
    assert!(next.is_some());
    let mut tiles = Vec::new();
    for shape in &first.shapes {
        if let egui::Shape::Rect(rect) = &shape.shape {
            if rect.fill != egui::Color32::TRANSPARENT && slot.contains_rect(rect.rect) {
                tiles.push(rect.rect);
            }
        }
    }
    tiles.sort_by(|a, b| a.left().total_cmp(&b.left()));
    assert_eq!(tiles.len(), 4);
    assert!(tiles
        .windows(2)
        .all(|pair| pair[0].right() <= pair[1].left()));
    let digits: Vec<_> = payroll_numbers(&first)
        .map(|(text, _)| text)
        .filter(|text| text.galley.text() != ".")
        .collect();
    assert_eq!(digits.len(), tiles.len());
    assert!(digits
        .iter()
        .zip(&tiles)
        .all(|(text, tile)| tile.contains(text.pos + text.galley.mesh_bounds.center().to_vec2())));

    let horizontal_amount = |frame: &egui::FullOutput| {
        let mut figures: Vec<_> = payroll_numbers(frame).map(|(text, _)| text).collect();
        figures.sort_by(|a, b| a.pos.x.total_cmp(&b.pos.x));
        figures
            .iter()
            .map(|text| text.galley.text())
            .collect::<String>()
    };
    assert_eq!(horizontal_amount(&first), "98.98");
    capture_payroll_pinned(&ctx, "live-pay-pinned-top.png", &warm, &first);
    let _ = payroll_frame(&ctx, &display, slot, false, 1.0);
    let (first_roll, _, animating) = payroll_frame(&ctx, &display, slot, false, 1.1);
    assert!(animating);
    let first_roll = horizontal_amount(&first_roll);
    assert!(
        first_roll.contains('8') && first_roll.contains('9'),
        "{first_roll}"
    );
    let (settled, _, animating) = payroll_frame(&ctx, &display, slot, false, 1.3);
    assert!(!animating);
    assert_eq!(horizontal_amount(&settled), "99.99");

    let _ = payroll_frame(&ctx, &display, slot, false, 2.0);
    let (second_roll, _, animating) = payroll_frame(&ctx, &display, slot, false, 2.1);
    assert!(animating);
    let second_roll = horizontal_amount(&second_roll);
    assert!(
        second_roll.contains('9') && second_roll.contains('0'),
        "{second_roll}"
    );
    let (settled, _, animating) = payroll_frame(&ctx, &display, slot, false, 2.3);
    assert!(!animating);
    assert_eq!(horizontal_amount(&settled), "101.00");

    for time in [2.5, 3.5, 4.5] {
        let _ = ctx.run(
            egui::RawInput {
                time: Some(time),
                ..Default::default()
            },
            |_| {},
        );
    }
    let (reopened, _, animating) = payroll_frame(&ctx, &display, slot, false, 5.0);
    assert!(!animating, "重新显示时从当前金额起步");
    assert_eq!(horizontal_amount(&reopened), "104.03");
}

#[test]
fn live_pay_side_pinned_shows_full_amount_at_readable_size() {
    let display = payroll_display(123456.78, 0.0);
    for (x, name) in [
        (18.0, "live-pay-pinned-left.png"),
        (126.0, "live-pay-pinned-right.png"),
    ] {
        let ctx = egui::Context::default();
        crate::theme::load_fonts(&ctx);
        let slot = egui::Rect::from_center_size(egui::pos2(x, 27.0), egui::vec2(36.0, 42.0));
        let (warm, _, _) = payroll_frame(&ctx, &display, slot, true, 0.0);
        let (frame, _, animating) = payroll_frame(&ctx, &display, slot, true, 0.02);
        assert!(!animating);
        capture_payroll_pinned(&ctx, name, &warm, &frame);
        let mut number: Vec<_> = payroll_numbers(&frame).collect();
        number.sort_by(|(a, _), (b, _)| {
            let row = |text: &egui::epaint::TextShape| {
                let y = text.pos.y + text.galley.mesh_bounds.center().y;
                (y / 7.0).round() as i32
            };
            row(a).cmp(&row(b)).then(a.pos.x.total_cmp(&b.pos.x))
        });
        assert_eq!(
            number
                .iter()
                .map(|(text, _)| text.galley.text())
                .collect::<String>(),
            "123456.78"
        );
        let mut integer_digits = 0;
        for (text, clip) in number {
            let painted = text.galley.mesh_bounds.translate(text.pos.to_vec2());
            assert!(
                slot.expand(0.5).contains_rect(painted),
                "{painted:?} 超出 {slot:?}"
            );
            assert!(
                clip.expand(0.5).contains_rect(painted),
                "{painted:?} 超出裁剪 {clip:?}"
            );
            if text.galley.job.sections[0].format.font_id.size >= 12.0 {
                integer_digits += text.galley.text().len();
            }
        }
        assert!(integer_digits >= 6, "整数部分应以至少 12 点字号绘制");
    }
}
