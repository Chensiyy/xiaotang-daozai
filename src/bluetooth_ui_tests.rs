use super::*;
use crate::{bt::{Device,DeviceKind,BatteryReading,BatterySource},app::BLUETOOTH_PAGE};

fn fixture()->Vec<Device> {
    [("耳机",DeviceKind::Headphones,Some(82)),("MX Master 3S",DeviceKind::Mouse,Some(18)),("MX Keys Mini",DeviceKind::Keyboard,Some(64)),("桌面音箱",DeviceKind::Speaker,None)].into_iter().enumerate().map(|(i,(name,kind,battery))|Device{
        id:format!("device-{i}"),name:name.into(),kind,battery,reading:battery.map(|percent|BatteryReading{percent,source:BatterySource::Gatt,at:std::time::Instant::now(),charging:None}),..Default::default()
    }).collect()
}

#[test]
fn wireless_monitor_survives_disabled_notifications_and_pins_persist(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();h.bt=fixture();h.app.settings.bluetooth_notifications=false;
    h.app.show_page(BLUETOOTH_PAGE);h.hover(H::local(200.0,100.0),1.0);
    assert_eq!(h.app.wireless_devices.len(),4);assert!(h.app.notice.is_none());
    let pin=h.ctx.read_response(egui::Id::new(("card",true,("wireless-pin","device-2")))).expect("设备卡片应提供常驻固定入口");
    h.click(pin.rect.center());
    assert_eq!(h.app.settings.bluetooth_pinned,vec!["device-2"]);
    assert_eq!(crate::model::Settings::load().bluetooth_pinned,vec!["device-2"]);
    assert_eq!(h.app.wireless_resident()[0].id,"device-2");
    h.app.settings.show_bluetooth=false;h.frame(vec![]);assert!(h.app.wireless_resident().is_empty());
}

#[test]
fn wireless_history_does_not_suppress_disconnect_notifications(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();h.bt=fixture();h.frame(vec![]);
    h.bt[0].connected=false;h.t+=3.0;h.frame(vec![]);
    assert!(h.app.notice.as_ref().is_some_and(|a|a.bluetooth==Some((DeviceKind::Headphones,false))));
    assert_eq!(h.app.wireless_devices.len(),4);assert_eq!(h.app.bt_shown.len(),3);
}

#[test]
fn wireless_cards_show_unknown_stale_and_second_page(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();h.bt=fixture();h.app.settings.bluetooth_notifications=false;
    h.bt[0].reading.as_mut().unwrap().at-=std::time::Duration::from_secs(181);
    let mut fifth=h.bt[1].clone();fifth.id="fifth".into();fifth.name="第五台设备".into();h.bt.push(fifth);
    h.app.show_page(BLUETOOTH_PAGE);h.hover(H::local(200.0,100.0),1.0);
    let texts:Vec<_>=h.shapes.iter().filter_map(|s|if let egui::Shape::Text(t)=&s.shape{Some(t.galley.text())}else{None}).collect();
    assert!(texts.contains(&"上次 82%"));assert!(texts.contains(&"未上报电量"));assert!(texts.contains(&"1 / 2"));
    h.click(H::local(352.0,212.0));assert_eq!(h.app.wireless_offset,1);
    h.bt.truncate(2);h.frame(vec![]);assert_eq!(h.app.wireless_offset,0);
}

#[test]
fn wireless_horizontal_cards_distinguish_battery_states(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();h.bt=fixture();h.app.settings.bluetooth_notifications=false;
    h.bt[0].battery=Some(4);h.bt[0].reading.as_mut().unwrap().percent=4;
    h.bt[2].reading.as_mut().unwrap().charging=Some(true);
    h.app.show_page(BLUETOOTH_PAGE);h.hover(H::local(200.0,100.0),1.0);
    let texts:Vec<_>=h.shapes.iter().filter_map(|s|if let egui::Shape::Text(t)=&s.shape{Some(t.galley.text())}else{None}).collect();
    for label in ["即将耗尽","电量偏低","正在充电","未上报电量"]{assert!(texts.contains(&label),"缺少状态：{label}");}
    let boxes:Vec<_>=(0..4).map(|i|h.ctx.read_response(egui::Id::new(("card",true,("wireless-device",format!("device-{i}"))))).unwrap().rect).collect();
    assert!(boxes.windows(2).all(|pair|(pair[0].top()-pair[1].top()).abs()<0.1&&pair[0].right()<pair[1].left()),"四台设备必须在同一行且互不重叠");
    h.bt[2].reading.as_mut().unwrap().at-=std::time::Duration::from_secs(181);h.frame(vec![]);
    assert!(!h.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t) if t.galley.text()=="正在充电")),"过期读数不能继续显示充电状态");
}

#[test]
fn wireless_native_preview(){
    let Some(dest)=std::env::var_os("DAOZAI_BLUETOOTH_PREVIEW_DIR") else{return;};
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let dest=std::path::PathBuf::from(dest);std::fs::create_dir_all(&dest).unwrap();
    for (name,skin) in [("native-dark.png",crate::model::Skin::Classic),("native-cream.png",crate::model::Skin::Cream)]{
        let mut h=H::new();h.app.settings.skin=skin;h.app.settings.bluetooth_notifications=false;h.bt=fixture();h.bt[2].reading.as_mut().unwrap().charging=Some(true);h.app.show_page(BLUETOOTH_PAGE);
        let mut raster=crate::preview_raster::Raster::new(640,250);
        for frame in 0..65 {
            let env=crate::app::Env{t:frame as f64/60.0,clock:"10:24:00".into(),date:"11月18日 周三".into(),bt:h.bt.clone(),bt_ready:true,cursor:Some(H::local(200.0,100.0)),..Default::default()};
            let out=h.ctx.run(egui::RawInput{screen_rect:Some(Rect::from_min_size(Pos2::ZERO,egui::vec2(640.0,250.0))),..Default::default()},|ctx|{egui::CentralPanel::default().frame(egui::Frame::new()).show(ctx,|ui|{h.app.frame(ui,Rect::from_min_size(Pos2::ZERO,egui::vec2(WIN_W,WIN_H)),&env);});});
            raster.update_textures(&out.textures_delta);
            if frame==64 {raster.buf.fill([0.15,0.17,0.21,1.0]);raster.draw(&h.ctx.tessellate(out.shapes,1.0),1.0);}
        }
        let mut bytes=vec![];raster.to_rgba8(&mut bytes);image::save_buffer(dest.join(name),&bytes,640,250,image::ColorType::Rgba8).unwrap();
    }
}
