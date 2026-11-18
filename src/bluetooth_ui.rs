//! 无线设备卡片、电量环与常驻入口。
use crate::{app::{App,Env,BLUETOOTH_PAGE},bt::Device,paint::{self,Card,H},theme::{self,W},bar_layout};
use std::{f32::consts::{FRAC_PI_2,TAU},time::Instant};

fn charging(device:&Device)->bool {
    !device.stale_at(Instant::now())&&device.reading.as_ref().is_some_and(|r|r.charging==Some(true))
}
fn bolt(c:&Card,x:f32,y:f32,col:eframe::egui::Color32){
    c.path(&[(x+3.0,y-5.0),(x-2.5,y+0.5),(x+2.0,y+0.5),(x-2.0,y+5.0)],1.5,col);
}
fn color(device:&Device)->eframe::egui::Color32 {
    if device.battery.is_none()||device.stale_at(Instant::now()) {theme::DIM}
    else if charging(device) {theme::SOFT_GREEN}
    else if device.battery.is_some_and(|b|b<=5) {theme::RED}
    else if device.battery.is_some_and(|b|b<20) {theme::YELLOW}
    else {theme::CYAN}
}
fn label(device:&Device)->String {
    let value=device.battery.map(|v|format!("{v}%")).unwrap_or_else(||"—".into());
    if device.stale_at(Instant::now())&&device.battery.is_some(){format!("上次 {value}")}else{value}
}
impl App {
    pub(crate) fn wireless_resident(&self)->Vec<Device> {
        if !self.settings.show_bluetooth||!self.module_on(BLUETOOTH_PAGE){return vec![];}
        let mut devices:Vec<_>=self.wireless_devices.iter().filter(|d|d.connected).cloned().collect();
        devices.sort_by_key(|d|(self.settings.bluetooth_pinned.iter().position(|id|id==&d.id).unwrap_or(2),d.stale_at(Instant::now()),d.battery.unwrap_or(101)));
        devices.truncate(2);devices
    }
    pub(crate) fn draw_wireless_bar(&mut self,c:&Card,cx:f32,layout:&bar_layout::Layout) {
        let Some(x)=layout.x[bar_layout::BLUETOOTH] else{return;};
        let mut devices=self.wireless_resident();devices.truncate((layout.widths[bar_layout::BLUETOOTH]/51.0) as usize);let start=cx+x-devices.len() as f32*25.5;
        for (i,device) in devices.iter().enumerate(){
            let x=start+i as f32*51.0;
            crate::extras::device_icon(c,device.kind,x,7.5,15.0);
            if charging(device){bolt(c,x+13.0,8.0,theme::SOFT_GREEN);}
            let text=if device.stale_at(Instant::now()){"—".into()}else{label(device)};
            c.text(x+19.0,19.0,&text,10.5,W::Semibold,color(device),H::Left);
            if c.hit(x-2.0,2.0,49.0,26.0,("wireless-resident",&device.id)).on_hover_text(format!("{} · {}",device.name,label(device))).clicked(){self.show_page(BLUETOOTH_PAGE);}
        }
    }
    pub(crate) fn draw_wireless_side(&mut self,c:&Card,cx:f32,y:f32) {
        let Some(device)=self.wireless_resident().into_iter().next() else{return;};
        crate::extras::device_icon(c,device.kind,cx-8.0,y-14.0,16.0);
        if charging(&device){bolt(c,cx+10.0,y-10.0,theme::SOFT_GREEN);}
        let text=if device.stale_at(Instant::now()){"—".into()}else{label(&device)};
        c.text(cx,y+15.0,&text,9.5,W::Semibold,color(&device),H::Center);
        if c.hit(cx-17.0,y-18.0,34.0,37.0,"wireless-side").on_hover_text(&device.name).clicked(){self.show_page(BLUETOOTH_PAGE);}
    }
    pub(crate) fn page_wireless(&mut self,c:&Card,env:&Env) {
        let devices=self.wireless_devices.clone();
        if devices.is_empty(){
            crate::extras::device_icon(c,crate::bt::DeviceKind::Other,191.5,51.0,36.0);
            c.text(209.5,112.0,if env.bt_ready{"还没有连接的设备"}else{"正在读取无线设备"},13.0,W::Semibold,theme::WHITE,H::Center);
            c.text(209.5,135.0,"连接耳机、鼠标或键盘后在这里查看",10.5,W::Regular,theme::DIM,H::Center);return;
        }
        let pages=devices.len().div_ceil(4);self.wireless_offset=self.wireless_offset.min(pages-1);
        let count=(devices.len()-self.wireless_offset*4).min(4);
        let start=(crate::app::PW-(count as f32*78.0+(count-1) as f32*9.0))/2.0;
        for (index,device) in devices.iter().skip(self.wireless_offset*4).take(4).enumerate(){
            let x=start+index as f32*87.0;let cc=c.shifted(x,42.0);
            let col=color(device);let stale=device.stale_at(Instant::now());let charging=charging(device);
            let attention=!stale&&(charging||device.battery.is_some_and(|v|v<20));
            cc.fill(0.0,0.0,78.0,118.0,13.0,theme::CARD);
            if attention {cc.fill(0.0,0.0,78.0,118.0,13.0,paint::alpha(col,0.045));}
            cc.stroke(0.0,0.0,78.0,118.0,13.0,0.7,paint::alpha(if attention{col}else{theme::GRAY},0.15));
            cc.circle(39.0,32.0,18.0,paint::alpha(col,0.035));
            cc.ring(39.0,32.0,21.0,2.5,theme::TRACK);
            if let Some(level)=device.battery.filter(|v|*v>0){cc.arc(39.0,32.0,21.0,-FRAC_PI_2,-FRAC_PI_2+TAU*level as f32/100.0,2.5,paint::alpha(col,if stale{0.45}else{1.0}));}
            crate::extras::device_icon(&cc,device.kind,28.0,21.0,22.0);
            if charging {cc.circle(55.0,47.0,8.0,theme::CARD);bolt(&cc,55.0,47.0,col);}
            cc.text(39.0,74.0,&label(device),if stale{10.0}else{20.0},W::Semibold,col,H::Center);
            let name=cc.fit(&device.name,10.0,W::Semibold,66.0);
            cc.text(39.0,91.0,&name,10.0,W::Semibold,theme::WHITE,H::Center);
            let status=if !device.connected{"已断开"}else if stale{"读数待更新"}else if device.battery.is_none(){"未上报电量"}else if charging{"正在充电"}else if device.battery.is_some_and(|b|b<=5){"即将耗尽"}else if device.battery.is_some_and(|b|b<20){"电量偏低"}else{"已连接"};
            if attention {cc.fill(9.0,98.0,60.0,16.0,8.5,paint::alpha(col,0.10));}
            cc.text(39.0,109.0,status,9.0,W::Regular,if attention{col}else{theme::DIM},H::Center);
            let pinned=self.settings.bluetooth_pinned.contains(&device.id);
            cc.ring(66.0,11.0,2.5,1.0,if pinned{theme::CYAN}else{theme::DIM});
            if pinned{cc.circle(66.0,11.0,1.2,theme::CYAN);}
            let pin=cc.hit(54.0,0.0,24.0,23.0,("wireless-pin",&device.id)).on_hover_text(if pinned{"取消优先常驻"}else{"优先常驻（最多两台）"});
            if pin.clicked(){
                if pinned {self.settings.bluetooth_pinned.retain(|id|id!=&device.id);}
                else {if self.settings.bluetooth_pinned.len()==2{self.settings.bluetooth_pinned.remove(0);}self.settings.bluetooth_pinned.push(device.id.clone());}
                self.settings.save();
            }
            let details=device.reading.as_ref().map(|r|format!("{}\n{} · {} 秒前读取",device.name,r.source.label(),r.at.elapsed().as_secs())).unwrap_or_else(||format!("{}\n设备尚未提供可用电量",device.name));
            cc.hit(0.0,24.0,78.0,94.0,("wireless-device",&device.id)).on_hover_text(details);
        }
        let count=devices.iter().filter(|d|d.connected).count();
        c.text(42.0,178.0,&format!("{count} 台已连接"),9.5,W::Regular,theme::DIM,H::Left);
        if pages>1 {
            c.text(322.0,178.0,&format!("{} / {pages}",self.wireless_offset+1),9.5,W::Regular,theme::GRAY,H::Center);
            for (x,next,text) in [(284.0,false,"‹"),(352.0,true,"›")] {
                c.text(x,179.0,text,15.0,W::Semibold,theme::WHITE,H::Center);
                if c.hit(x-12.0,165.0,24.0,20.0,("wireless-page",next)).clicked(){self.wireless_offset=(self.wireless_offset+if next{1}else{pages-1})%pages;}
            }
        }else{c.text(377.0,178.0,"自动更新",9.5,W::Regular,theme::DIM,H::Right);}
    }
}
