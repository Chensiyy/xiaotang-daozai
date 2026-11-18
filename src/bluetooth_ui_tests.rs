use super::*;
use crate::{bt::{Device,DeviceKind,BatteryReading,BatterySource},app::BLUETOOTH_PAGE};

fn fixture()->Vec<Device> {
    [("耳机",DeviceKind::Headphones,Some(82)),("MX Master 3S",DeviceKind::Mouse,Some(18)),("MX Keys Mini",DeviceKind::Keyboard,Some(64)),("桌面音箱",DeviceKind::Speaker,None)].into_iter().enumerate().map(|(i,(name,kind,battery))|Device{
        id:format!("device-{i}"),name:name.into(),kind,battery,reading:battery.map(|percent|BatteryReading{percent,source:BatterySource::Gatt,at:std::time::Instant::now(),charging:None}),..Default::default()
    }).collect()
}
fn texts(h:&H)->Vec<String> {h.shapes.iter().filter_map(|s|if let egui::Shape::Text(t)=&s.shape{Some(t.galley.text().to_string())}else{None}).collect()}
fn open_page(h:&mut H) {h.app.show_page(BLUETOOTH_PAGE);h.hover(H::local(200.0,100.0),1.0);}
fn set_level(d:&mut Device,level:u8) {d.battery=Some(level);d.reading.as_mut().unwrap().percent=level;}

#[test]
fn wireless_custom_resident_persists_from_detail_toggle(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();h.bt=fixture();h.app.settings.bluetooth_notifications=false;
    open_page(&mut h);
    assert_eq!(h.app.wireless_devices.len(),4);assert!(h.app.notice.is_none());
    // 自动模式：只挑有电量读数的设备，电量低的在前，最多两台。
    assert_eq!(h.app.wireless_resident().iter().map(|d|d.id.as_str()).collect::<Vec<_>>(),["device-1","device-2"]);
    let tile=h.ctx.read_response(egui::Id::new(("card",true,("wireless-device","device-0")))).unwrap();
    h.click(tile.rect.center());h.frame(vec![]);
    let pin=h.ctx.read_response(egui::Id::new(("card",true,("wireless-pin","device-0")))).expect("详情应提供常驻显示开关");
    h.click(pin.rect.center());
    assert!(h.app.settings.bluetooth_resident_custom);
    assert_eq!(h.app.settings.bluetooth_pinned,vec!["device-0"]);
    let saved=crate::model::Settings::load();
    assert!(saved.bluetooth_resident_custom);assert_eq!(saved.bluetooth_pinned,vec!["device-0"]);
    assert_eq!(h.app.wireless_resident().iter().map(|d|d.id.as_str()).collect::<Vec<_>>(),["device-0"]);
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
fn wireless_rings_show_unknown_compact_and_paging(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();h.bt=fixture();h.app.settings.bluetooth_notifications=false;
    for i in 4..6 {let mut d=h.bt[1].clone();d.id=format!("device-{i}");d.name=format!("第 {i} 台");h.bt.push(d);}
    open_page(&mut h);
    let t=texts(&h);assert!(t.iter().any(|s|s=="···"),"未上报电量用小点占位");assert!(!t.iter().any(|s|s.contains(" / ")),"6 台以内一页放下");
    let boxes:Vec<_>=(0..6).map(|i|h.ctx.read_response(egui::Id::new(("card",true,("wireless-device",format!("device-{i}"))))).unwrap().rect).collect();
    assert!(boxes.windows(2).all(|p|(p[0].top()-p[1].top()).abs()<0.1&&p[0].right()<=p[1].left()+0.1),"紧凑排法仍在同一行且不重叠");
    let mut d=h.bt[1].clone();d.id="device-6".into();h.bt.push(d);h.frame(vec![]);h.frame(vec![]);
    assert!(texts(&h).iter().any(|s|s=="1 / 2"),"7 台以上每页 4 台");
    h.click(H::local(348.0,175.0));assert_eq!(h.app.wireless_offset,1);
    h.bt.truncate(2);h.frame(vec![]);assert_eq!(h.app.wireless_offset,0);
}

#[test]
fn wireless_detail_marks_charging_with_text_only_when_fresh(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();h.bt=fixture();h.app.settings.bluetooth_notifications=false;
    h.bt[2].reading.as_mut().unwrap().charging=Some(true);
    open_page(&mut h);h.app.wireless_focus=Some("device-2".into());h.frame(vec![]);
    assert!(texts(&h).iter().any(|s|s=="充电中"));
    h.bt[2].reading.as_mut().unwrap().at-=std::time::Duration::from_secs(181);h.frame(vec![]);
    assert!(!texts(&h).iter().any(|s|s=="充电中"),"过期读数不能继续显示充电状态");
}

#[test]
fn wireless_rings_stay_centered_with_two_to_four_devices(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    for count in 2..=4 {
        let mut h=H::new();h.bt=fixture();h.bt.truncate(count);h.app.settings.bluetooth_notifications=false;
        open_page(&mut h);
        let boxes:Vec<_>=(0..count).map(|i|h.ctx.read_response(egui::Id::new(("card",true,("wireless-device",format!("device-{i}"))))).unwrap().rect).collect();
        let center=(boxes.first().unwrap().left()+boxes.last().unwrap().right())/2.0;
        assert!((center-H::local(PW/2.0,0.0).x).abs()<0.5,"{count} 台设备应整组居中");
    }
    let mut h=H::new();h.bt=fixture();h.bt.truncate(1);h.app.settings.bluetooth_notifications=false;open_page(&mut h);
    assert!(h.ctx.read_response(egui::Id::new(("card",true,("wireless-pin","device-0")))).is_some(),"只有一台时直接显示详情");
    assert!(h.ctx.read_response(egui::Id::new(("card",true,"wireless-back"))).is_none(),"单台详情不需要返回");
}

#[test]
fn wireless_device_opens_and_returns_inside_island(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();h.bt=fixture();open_page(&mut h);
    let tile=h.ctx.read_response(egui::Id::new(("card",true,("wireless-device","device-1")))).unwrap();
    h.click(tile.rect.center());
    assert_eq!(h.app.wireless_focus.as_deref(),Some("device-1"));
    h.frame(vec![]);
    let back=h.ctx.read_response(egui::Id::new(("card",true,"wireless-back"))).unwrap();
    h.click(back.rect.center());
    assert!(h.app.wireless_focus.is_none());
    assert_eq!(h.app.page(),BLUETOOTH_PAGE);
}

#[test]
fn ring_only_resident_shows_numbers_for_low_or_charging(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();h.bt=fixture();h.app.settings.bluetooth_notifications=false;
    h.app.settings.bluetooth_resident_custom=true;h.app.settings.bluetooth_pinned=vec!["device-0".into(),"device-1".into()];
    h.app.settings.bluetooth_resident_style=1;set_level(&mut h.bt[1],15);
    h.frame(vec![]);
    let t=texts(&h);assert!(t.iter().any(|s|s=="15%"),"偏低的设备显示数字");assert!(!t.iter().any(|s|s=="82%"),"电量充足的只显示环");
    let narrow=h.app.wireless_bar_width();
    h.app.settings.bluetooth_resident_style=0;h.frame(vec![]);
    assert!(texts(&h).iter().any(|s|s=="82%"));assert!(h.app.wireless_bar_width()>narrow);
}

#[test]
fn battery_events_notify_once_and_respect_setting(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();h.bt=fixture();h.app.settings.bluetooth_notifications=false;
    let sent=|h:&H,body:&str|h.app.history.iter().filter(|a|a.body==body).count();
    set_level(&mut h.bt[1],21);h.frame(vec![]);
    set_level(&mut h.bt[1],19);h.t+=1.0;h.frame(vec![]);
    assert_eq!(sent(&h,"MX Master 3S · 电量偏低 · 19%"),1,"跌入偏低时提示");
    assert!(h.app.wireless_fx.get("device-1").is_some_and(|fx|fx.nudge.is_some()&&fx.morph.is_some_and(|m|m.0==21)));
    set_level(&mut h.bt[1],21);h.t+=3.0;h.frame(vec![]);
    set_level(&mut h.bt[1],19);h.t+=3.0;h.frame(vec![]);
    assert_eq!(sent(&h,"MX Master 3S · 电量偏低 · 19%"),1,"10 分钟内不重复提示");
    h.bt[2].reading.as_mut().unwrap().charging=Some(true);h.t+=1.0;h.frame(vec![]);
    assert_eq!(sent(&h,"MX Keys Mini · 开始充电 · 64%"),1);
    h.app.settings.bluetooth_battery_notifications=false;
    h.bt[0].reading.as_mut().unwrap().charging=Some(true);h.t+=1.0;h.frame(vec![]);
    assert_eq!(sent(&h,"耳机 · 开始充电 · 82%"),0,"关闭电量提示后不再提示");
}

#[test]
fn wireless_native_preview(){
    let Some(dest)=std::env::var_os("DAOZAI_BLUETOOTH_PREVIEW_DIR") else{return;};
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let dest=std::path::PathBuf::from(dest);std::fs::create_dir_all(&dest).unwrap();
    for (name,skin) in [("native-dark.png",crate::model::Skin::Classic),("native-cream.png",crate::model::Skin::Cream),("native-resident.png",crate::model::Skin::Classic),("native-detail.png",crate::model::Skin::Classic)]{
        let mut h=H::new();h.app.settings.skin=skin;h.app.settings.bluetooth_notifications=false;h.bt=fixture();h.bt[2].reading.as_mut().unwrap().charging=Some(true);
        h.bt[0].kind=DeviceKind::EarbudsStemTips;h.bt[0].name="AirPods Pro".into();
        let resident=name=="native-resident.png";
        if resident {h.app.settings.bluetooth_resident_custom=true;h.app.settings.bluetooth_pinned=vec!["device-1".into(),"device-2".into()];}
        if !resident {h.app.show_page(BLUETOOTH_PAGE);}
        if name=="native-detail.png" {h.app.wireless_focus=Some(h.bt[2].id.clone());}
        let height=if resident{50}else{210};
        let mut raster=crate::preview_raster::Raster::new(640,height);
        for frame in 0..65 {
            let env=crate::app::Env{t:frame as f64/60.0,clock:"10:24:00".into(),date:"11月18日 周三".into(),bt:h.bt.clone(),bt_ready:true,cursor:if resident{None}else{Some(H::local(200.0,100.0))},..Default::default()};
            let out=h.ctx.run(egui::RawInput{screen_rect:Some(Rect::from_min_size(Pos2::ZERO,egui::vec2(640.0,height as f32))),..Default::default()},|ctx|{egui::CentralPanel::default().frame(egui::Frame::new()).show(ctx,|ui|{h.app.frame(ui,Rect::from_min_size(Pos2::ZERO,egui::vec2(WIN_W,WIN_H)),&env);});});
            raster.update_textures(&out.textures_delta);
            if frame==64 {raster.buf.fill([0.15,0.17,0.21,1.0]);raster.draw(&h.ctx.tessellate(out.shapes,1.0),1.0);}
        }
        let mut bytes=vec![];raster.to_rgba8(&mut bytes);image::save_buffer(dest.join(name),&bytes,640,height as u32,image::ColorType::Rgba8).unwrap();
    }
}
