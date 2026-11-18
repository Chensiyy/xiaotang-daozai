//! 无线设备电量环：展开页、详情、常驻条和侧边岛。
//! 动效只在展开时出现（充电流光、低电量呼吸、一次性事件）；常驻条保持静止，不抢注意力。
use crate::{anim,app::{App,Env,BLUETOOTH_PAGE},bt::{Device,DeviceKind},paint::{self,Card,H},theme::{self,W},bar_layout};
use eframe::egui::Color32;
use std::f32::consts::{PI,TAU};
use std::time::Instant;

/// 一次性事件的起始时间（秒，取 App::t）。
#[derive(Clone,Debug,Default)]
pub(crate) struct WirelessFx {
    /// 电量变化：从哪个百分比缓动过来。
    pub morph:Option<(u8,f64)>,
    pub plug:Option<f64>,
    pub unplug:Option<f64>,
    pub full:Option<f64>,
    /// 跌进橙色或红色时，图标左右轻晃。
    pub nudge:Option<f64>,
}
const FLOW:f64=1.8;
const ENTER:f32=0.9;
const STAGGER:f32=0.07;

pub(crate) fn charging(device:&Device)->bool {
    !device.stale_at(Instant::now())&&device.reading.as_ref().is_some_and(|r|r.charging==Some(true))
}
fn stale(device:&Device)->bool {device.stale_at(Instant::now())}
/// 绿 ≥50 · 黄 20–49 · 橙 10–19 · 红 <10。
pub(crate) fn level_color(level:u8)->Color32 {
    if level>=50 {theme::GREEN} else if level>=20 {theme::YELLOW} else if level>=10 {theme::ORANGE} else {theme::RED}
}
fn band(level:u8)->u8 {if level>=50 {3} else if level>=20 {2} else if level>=10 {1} else {0}}
fn color(device:&Device)->Color32 {
    match device.battery {Some(level) if !stale(device)=>level_color(level),_=>theme::DIM}
}
/// 偏低（橙、红）且没在充电：展开时呼吸，常驻条“只显示环”时亮出数字。
fn low(device:&Device)->bool {!stale(device)&&!charging(device)&&device.battery.is_some_and(|b|b<20)}

/// 闪电由两个三角拼成（填充多边形只支持凸形）。设计稿坐标系 12×16。
fn bolt(c:&Card,cx:f32,cy:f32,h:f32,col:Color32) {
    if h<=0.2 {return;}
    let s=h/16.0;let p=|x:f32,y:f32|(cx+(x-6.0)*s,cy+(y-8.0)*s);
    c.poly(&[p(7.6,0.6),p(1.2,9.0),p(7.0,9.0)],col);
    c.poly(&[p(4.2,15.4),p(10.8,6.8),p(5.0,6.8)],col);
}
/// 充电标识的缩放：接电时弹出、拔电时缩回；展开时跟流光同拍跳一下。
fn bolt_scale(device:&Device,fx:Option<&WirelessFx>,t:f64,live:bool)->f32 {
    let fx=fx.cloned().unwrap_or_default();
    if !charging(device) {
        return fx.unplug.map_or(0.0,|at|{let k=((t-at)/0.35) as f32;if k<1.0 {1.0-anim::clamp01(k)}else{0.0}});
    }
    if let Some(at)=fx.plug {
        let k=((t-at)/0.55) as f32;
        if (0.0..1.0).contains(&k) {return if k<0.6 {k/0.6*1.35}else{1.35-0.35*(k-0.6)/0.4};}
    }
    if !live||device.battery==Some(100) {return 1.0;}
    let u=((t%FLOW)/FLOW) as f32;
    if u<0.82 {1.0} else if u<0.91 {1.0+0.22*(u-0.82)/0.09} else {1.22-0.22*(u-0.91)/0.09}
}

struct RingSpec {r:f32,w:f32,icon:f32,live:bool,enter:f32,hover:bool}

/// 弧线从 12 点钟顺时针。展开时叠加：低电量呼吸、充电流光与弧端闪点、接电/充满波纹、充满扫光、跌档晃动。
fn ring(c:&Card,cx:f32,cy:f32,spec:&RingSpec,device:&Device,fx:Option<&WirelessFx>,t:f64) {
    let rr=spec.r-spec.w/2.0;let chg=charging(device);
    let mut track=if spec.hover {paint::mix(theme::TRACK,theme::WHITE,0.18)} else {theme::TRACK};
    if spec.live&&device.battery.is_some_and(|b|b<10)&&!stale(device)&&!chg {track=paint::mix(track,theme::RED,0.22);}
    match device.battery {
        None=>for i in 0..24 {let a=i as f32/24.0*TAU;c.arc(cx,cy,rr,a,a+TAU/24.0*0.42,spec.w*0.8,track);},
        Some(_)=>c.ring(cx,cy,rr,spec.w,track),
    }
    let target=device.battery.map_or(0.0,|b|b as f32/100.0);
    let mut frac=target;let mut col=color(device);
    if let (Some(fx),Some(level)) = (fx,device.battery) {
        if let Some((from,at))=fx.morph {
            let k=anim::clamp01(((t-at)/0.8) as f32);
            if k<1.0 {
                let e=anim::ease_out_cubic(k);frac=from as f32/100.0+(target-from as f32/100.0)*e;
                if !stale(device) {col=paint::mix(level_color(from),level_color(level),e);}
            }
        }
    }
    frac*=anim::ease_out_cubic(spec.enter);
    if spec.live&&low(device) {
        // 呼吸用混向轨道色代替透明度，弧线和圆头叠在一起时不会出现深色接缝。
        let period=if device.battery.is_some_and(|b|b<10) {1.3} else {2.6};
        let k=0.5-0.5*((t/period) as f32*TAU).cos();col=paint::mix(col,track,0.6*k);
    }
    let a0=-PI/2.0;let a1=a0+TAU*frac;
    if frac>0.002 {
        c.arc(cx,cy,rr,a0,a1,spec.w,col);
        for a in [a0,a1] {c.circle(cx+rr*a.cos(),cy+rr*a.sin(),spec.w/2.0,col);}
    }
    if let Some(fx)=fx.filter(|_|spec.live) {
        for at in [fx.plug,fx.full].into_iter().flatten() {
            for i in 0..2 {
                let k=((t-at) as f32-i as f32*0.9)/0.9;
                if (0.0..1.0).contains(&k) {c.ring(cx,cy,spec.r*(0.95+0.5*k),(spec.w*0.5).max(1.0),paint::alpha(col,0.85*(1.0-k)));}
            }
        }
        if let Some(at)=fx.full {
            let k=((t-at)/1.1) as f32;
            if (0.0..1.0).contains(&k) {let e=anim::ease_in_out(k);let s=a0+e*TAU;c.arc(cx,cy,rr,s,s+0.14*TAU,spec.w,paint::alpha(paint::mix(col,theme::WHITE,0.7),1.0-k));}
        }
    }
    if spec.live&&chg&&frac<0.999&&spec.enter>=1.0&&frac>0.02 {
        let span=TAU*frac;let seg=(0.1*TAU).min(span*0.5);
        let u=((t%FLOW)/FLOW) as f32;let e=anim::ease_in_out(u);
        let fade=anim::smoothstep(0.0,0.15,u)*(1.0-anim::smoothstep(0.85,1.0,u))*0.95;
        let s=a0+e*(span-seg);
        c.arc(cx,cy,rr,s,s+seg,spec.w,paint::alpha(paint::mix(col,theme::WHITE,0.65),fade));
        if u>0.8 {
            let k=(u-0.8)/0.2;let (scale,op)=if k<0.5 {(0.3+1.2*k/0.5,k/0.5)} else {(1.5-0.5*(k-0.5)/0.5,1.0-(k-0.5)/0.5)};
            c.circle(cx+rr*a1.cos(),cy+rr*a1.sin(),spec.w*0.55*scale,paint::alpha(paint::mix(col,theme::WHITE,0.6),op));
        }
    }
    let mut dx=0.0;
    if let Some(at)=fx.and_then(|fx|fx.nudge).filter(|_|spec.live) {
        let e=(t-at) as f32;if (0.0..1.2).contains(&e) {dx=2.5*(e/0.6*TAU).sin()*(1.0-e/1.2);}
    }
    let tint=if stale(device)||!device.connected {theme::DIM} else {theme::WHITE};
    crate::extras::device_icon_tinted(c,device.kind,cx-spec.icon/2.0+dx,cy-spec.icon/2.0,spec.icon,tint);
}

/// 百分比（充电时前面带闪电）。centered 时整组以 x 居中，否则从 x 左对齐。返回总宽。
fn reading(c:&Card,x:f32,cy:f32,size:f32,device:&Device,fx:Option<&WirelessFx>,t:f64,live:bool,enter:f32,centered:bool)->f32 {
    let col=color(device);
    let text=match device.battery {
        None=>"···".to_string(),
        Some(level)=>{
            let mut shown=level as f32;
            if let Some((from,at))=fx.and_then(|fx|fx.morph) {
                let k=anim::clamp01(((t-at)/0.8) as f32);shown=from as f32+(level as f32-from as f32)*anim::ease_out_cubic(k);
            }
            format!("{}%",(shown*anim::ease_out_cubic(enter)).round() as u8)
        }
    };
    let weight=if device.battery.is_some()&&!stale(device) {W::Bold} else {W::Semibold};
    let tw=c.text_w(&text,size,weight);
    let scale=bolt_scale(device,fx,t,live);
    let bw=if scale>0.0 {size*0.62+2.0} else {0.0};
    let left=if centered {x-(tw+bw)/2.0} else {x};
    if scale>0.0 {bolt(c,left+size*0.31,cy,size*0.84*scale,col);}
    c.centered_text(left+bw,cy,&text,size,weight,col,H::Left);
    tw+bw
}

fn seconds_ago(device:&Device)->Option<String> {
    let secs=device.reading.as_ref()?.at.elapsed().as_secs();
    Some(if secs<60 {format!("{secs} 秒前读取")} else {format!("{} 分钟前读取",secs/60)})
}

impl App {
    /// 展开页入场进度：每台设备错开 70ms，0.9s 画完。
    fn wireless_enter(&self,index:usize)->f32 {
        self.wireless_enter_at.map_or(1.0,|at|anim::clamp01(((self.t-at) as f32-index as f32*STAGGER)/ENTER))
    }
    /// 展开页是否需要持续重绘：入场、充电流光、低电量呼吸或一次性事件进行中。
    pub(crate) fn wireless_animating(&self)->bool {
        if !(self.open&&self.page==BLUETOOTH_PAGE) {return false;}
        self.wireless_enter_at.is_some_and(|at|self.t-at<ENTER as f64+0.5)
            || self.wireless_devices.iter().any(|d|d.connected&&(charging(d)||low(d)))
            || self.wireless_fx.values().any(|fx|[fx.morph.map(|m|m.1),fx.plug,fx.unplug,fx.full,fx.nudge].into_iter().flatten().any(|at|self.t-at<2.0))
    }
    /// 对比新旧快照，记录一次性动效并按需在岛上提示。
    pub(crate) fn track_wireless_changes(&mut self,next:&[Device]) {
        let now=self.t;
        self.wireless_fx.retain(|_,fx|[fx.morph.map(|m|m.1),fx.plug,fx.unplug,fx.full,fx.nudge].into_iter().flatten().any(|at|now-at<3.0));
        let previous=std::mem::take(&mut self.wireless_devices);
        for device in next.iter().filter(|d|d.connected) {
            let Some(old)=previous.iter().find(|d|d.id==device.id&&d.connected) else {continue};
            let (was,is)=(charging(old),charging(device));
            let mut events=vec![];
            let fx=self.wireless_fx.entry(device.id.clone()).or_default();
            if !was&&is {fx.plug=Some(now);events.push("开始充电");}
            if was&&!is&&!stale(device) {fx.unplug=Some(now);}
            if let (Some(from),Some(to))=(old.battery,device.battery) {
                if from!=to&&!stale(device) {
                    fx.morph=Some((from,now));
                    if is&&to==100&&from<100 {fx.full=Some(now);events.push("已充满，可以拔掉了");}
                    if !is&&band(to)<band(from)&&to<20 {fx.nudge=Some(now);events.push(if to<10 {"电量即将耗尽，请尽快充电"} else {"电量偏低"});}
                }
            }
            for event in events {self.battery_alert(device,event);}
        }
        self.wireless_devices=previous;
    }
    /// 电量事件提示：同一台设备的同一件事 10 分钟内只提示一次。
    fn battery_alert(&mut self,device:&Device,event:&'static str) {
        if !self.settings.bluetooth_battery_notifications {return;}
        let key=(device.id.clone(),event);
        if self.wireless_alerted.get(&key).is_some_and(|at|self.t-at<600.0) {return;}
        self.wireless_alerted.insert(key,self.t);
        let Some(level)=device.battery else {return};
        self.enqueue_alert(crate::extras::Alert{
            source:crate::extras::AlertSource::Internal,title:"蓝牙设备".into(),
            body:format!("{} · {event} · {level}%",device.name),page:BLUETOOTH_PAGE,at:self.t,duration:2.6,
            bluetooth:Some((device.kind,true)),meeting:None,unread:None,
        },false);
    }

    /// 常驻候选：自动模式取有电量读数的设备，自定义只取勾选的；按电量从低到高，最多两台。
    pub(crate) fn wireless_resident(&self)->Vec<Device> {
        if !self.settings.show_bluetooth||!self.module_on(BLUETOOTH_PAGE){return vec![];}
        let custom=self.settings.bluetooth_resident_custom;
        let mut devices:Vec<_>=self.wireless_devices.iter().filter(|d|d.connected)
            .filter(|d|if custom {self.settings.bluetooth_pinned.contains(&d.id)} else {d.battery.is_some()}).cloned().collect();
        devices.sort_by_key(|d|(stale(d),d.battery.unwrap_or(101)));
        devices.truncate(2);devices
    }
    fn resident_value(&self,device:&Device)->bool {
        device.battery.is_some()&&(self.settings.bluetooth_resident_style==0||charging(device)||low(device))
    }
    fn resident_slot(&self,device:&Device)->f32 {
        if !self.resident_value(device) {return 18.0;}
        let digits=device.battery.map_or(1,|b|b.to_string().len()) as f32;
        18.0+3.0+if charging(device){8.0}else{0.0}+digits*5.8+8.0
    }
    /// 常驻条上无线设备一组的宽度（含设备之间 8 的间距）。
    pub(crate) fn wireless_bar_width(&self)->f32 {
        let devices=self.wireless_resident();
        devices.iter().map(|d|self.resident_slot(d)).sum::<f32>()+8.0*devices.len().saturating_sub(1) as f32
    }
    /// 常驻条：静止的小环 + 数字，不走流光、不呼吸。
    pub(crate) fn draw_wireless_bar(&mut self,c:&Card,cx:f32,layout:&bar_layout::Layout) {
        let Some(x)=layout.x[bar_layout::BLUETOOTH] else{return;};
        let devices=self.wireless_resident();let total=self.wireless_bar_width().min(layout.widths[bar_layout::BLUETOOTH]+0.5);
        let mut left=cx+x-total/2.0;
        for device in &devices {
            let w=self.resident_slot(device);if left+w>cx+x+total/2.0+0.5 {break;}
            let spec=RingSpec{r:9.0,w:2.0,icon:10.0,live:false,enter:1.0,hover:false};
            ring(c,left+9.0,15.0,&spec,device,None,self.t);
            if self.resident_value(device) {reading(c,left+21.0+(w-21.0)/2.0,15.5,9.5,device,None,self.t,false,1.0,true);}
            let tip=format!("{} · {}",device.name,device.battery.map_or("未上报电量".into(),|b|format!("{b}%")));
            if c.hit(left-2.0,2.0,w+4.0,26.0,("wireless-resident",&device.id)).on_hover_text(tip).clicked(){self.wireless_focus=Some(device.id.clone());self.show_page(BLUETOOTH_PAGE);}
            left+=w+8.0;
        }
    }
    pub(crate) fn draw_wireless_side(&mut self,c:&Card,cx:f32,y:f32) {
        let Some(device)=self.wireless_resident().into_iter().next() else{return;};
        let spec=RingSpec{r:11.0,w:2.2,icon:12.0,live:false,enter:1.0,hover:false};
        let value=self.resident_value(&device);
        let ring_y=if value {y-6.0} else {y};
        ring(c,cx,ring_y,&spec,&device,None,self.t);
        if value {reading(c,cx,y+14.0,8.5,&device,None,self.t,false,1.0,true);}
        if c.hit(cx-17.0,y-18.0,34.0,37.0,"wireless-side").on_hover_text(&device.name).clicked(){self.wireless_focus=Some(device.id.clone());self.show_page(BLUETOOTH_PAGE);}
    }

    pub(crate) fn page_wireless(&mut self,c:&Card,env:&Env) {
        if self.wireless_enter_at.is_none() {self.wireless_enter_at=Some(self.t);}
        let devices=self.wireless_devices.clone();
        // 只有一台时直接用详情版式，不让一个小环孤零零地待在中间。
        if devices.len()==1 {self.wireless_detail(c,&devices[0],false);return;}
        if let Some(id)=self.wireless_focus.clone() {
            if let Some(device)=devices.iter().find(|d|d.id==id) {
                self.wireless_detail(c,device,true);
                return;
            }
            self.wireless_focus=None;
        }
        if devices.is_empty(){
            crate::extras::device_icon(c,DeviceKind::Other,191.5,51.0,36.0);
            c.text(209.5,112.0,if env.bt_ready{"还没有连接的设备"}else{"正在读取无线设备"},13.0,W::Semibold,theme::WHITE,H::Center);
            c.text(209.5,135.0,"连接耳机、鼠标或键盘后在这里查看",10.5,W::Regular,theme::DIM,H::Center);return;
        }
        let per_page=if devices.len()<=6 {devices.len()} else {4};
        let pages=devices.len().div_ceil(per_page);self.wireless_offset=self.wireless_offset.min(pages-1);
        let shown:Vec<_>=devices.iter().skip(self.wireless_offset*per_page).take(per_page).collect();
        let compact=per_page>4;
        let (col_w,gap,r,w,icon,pct,name,ring_y,pct_y,name_y)=if compact {(64.0,4.0,23.0,3.5,19.0,13.0,9.5,78.0,118.0,136.0)} else {(82.0,12.0,29.0,4.0,24.0,15.0,10.0,80.0,127.0,146.0)};
        let count=shown.len() as f32;
        let start=(crate::app::PW-(count*col_w+(count-1.0)*gap))/2.0;
        for (index,device) in shown.iter().enumerate() {
            let x=start+index as f32*(col_w+gap);let cx=x+col_w/2.0;
            let details=device.reading.as_ref().map(|r|format!("{}\n{} · {}",device.name,r.source.label(),seconds_ago(device).unwrap_or_default())).unwrap_or_else(||format!("{}\n设备尚未提供可用电量",device.name));
            let hit=c.hit(x,ring_y-r-6.0,col_w,name_y-ring_y+r+14.0,("wireless-device",&device.id)).on_hover_text(details);
            let hover=hit.hovered();
            let enter=self.wireless_enter(index);let fx=self.wireless_fx.get(&device.id).cloned();
            let cc=if device.connected {c.clone()} else {c.faded(0.4)};
            ring(&cc,cx,ring_y,&RingSpec{r,w,icon,live:true,enter,hover},device,fx.as_ref(),self.t);
            let rise=(1.0-anim::ease_out_cubic(anim::clamp01((enter-0.25)/0.55)))*6.0;
            let tc=cc.faded(anim::clamp01((enter-0.25)/0.55));
            reading(&tc,cx,pct_y+rise,pct,device,fx.as_ref(),self.t,true,enter,true);
            let label=tc.fit(&device.name,name,W::Regular,col_w-2.0);
            tc.centered_text(cx,name_y+rise,&label,name,W::Regular,if hover{theme::WHITE}else{theme::DIM},H::Center);
            if hit.clicked() {self.wireless_focus=Some(device.id.clone());}
        }
        if pages>1 {
            c.text(322.0,178.0,&format!("{} / {pages}",self.wireless_offset+1),9.5,W::Regular,theme::GRAY,H::Center);
            for (x,next,text) in [(296.0,false,"‹"),(348.0,true,"›")] {
                c.text(x,179.0,text,15.0,W::Semibold,theme::WHITE,H::Center);
                if c.hit(x-12.0,165.0,24.0,20.0,("wireless-page",next)).clicked(){self.wireless_offset=(self.wireless_offset+if next{1}else{pages-1})%pages;}
            }
        }
    }
    /// 详情：环、名称、读数与总览同一顺序，只是放大。有返回入口时标题前带“‹”。
    fn wireless_detail(&mut self,c:&Card,device:&Device,back:bool) {
        if back {
            let wa=c.text(15.0,25.0,"‹ 无线",crate::theme::size::TITLE,W::Bold,theme::WHITE,H::Left);
            c.text(15.0+wa+1.5,25.0,"设备",crate::theme::size::TITLE,W::Bold,theme::CYAN,H::Left);
            if c.hit(6.0,4.0,wa+46.0,30.0,"wireless-back").clicked(){self.wireless_focus=None;}
        }
        let fx=self.wireless_fx.get(&device.id).cloned();let enter=self.wireless_enter(0);
        ring(c,112.0,100.0,&RingSpec{r:50.0,w:6.0,icon:38.0,live:true,enter,hover:false},device,fx.as_ref(),self.t);
        let x=190.0;
        c.centered_text(x,66.0,&c.fit(&device.name,15.0,W::Bold,190.0),15.0,W::Bold,theme::WHITE,H::Left);
        reading(c,x,101.0,34.0,device,fx.as_ref(),self.t,true,enter,false);
        let mut mx=x;
        if charging(device) {mx+=c.text(mx,140.0,"充电中",10.5,W::Semibold,color(device),H::Left)+10.0;}
        let source=device.reading.as_ref().map_or("设备没有提供电量",|r|r.source.label());
        mx+=c.text(mx,140.0,source,10.5,W::Regular,theme::DIM,H::Left)+10.0;
        if let Some(ago)=seconds_ago(device) {c.text(mx,140.0,&ago,10.5,W::Regular,theme::DIM,H::Left);}
        // 常驻显示：和设置里的自定义列表是同一份数据。
        let on=self.settings.bluetooth_resident_custom&&self.settings.bluetooth_pinned.contains(&device.id);
        let (tx,ty)=(x,158.0);
        c.fill(tx,ty-7.0,24.0,14.0,7.0,if on{theme::CYAN}else{theme::TRACK});
        c.circle(if on{tx+17.0}else{tx+7.0},ty,5.0,if on{Color32::BLACK}else{theme::DIM});
        c.centered_text(tx+31.0,ty,"常驻显示",10.5,W::Regular,if on{theme::WHITE}else{theme::DIM},H::Left);
        let hint=if on {"取消常驻显示"} else {"在收起的岛上常驻显示这台设备"};
        if c.hit(tx-4.0,ty-11.0,92.0,22.0,("wireless-pin",&device.id)).on_hover_text(hint).clicked() {
            if on {self.settings.bluetooth_pinned.retain(|id|id!=&device.id);}
            else {
                if !self.settings.bluetooth_resident_custom {self.settings.bluetooth_pinned.clear();self.settings.bluetooth_resident_custom=true;}
                self.settings.bluetooth_pinned.push(device.id.clone());
            }
            self.settings.save();
        }
    }
}

impl App {
    /// 设置 › 页面管理 › 无线设备 下的两项：常驻样式、常驻设备（自动 / 自定义 + 勾选列表）。
    pub(crate) fn wireless_resident_settings(&mut self,ui:&mut eframe::egui::Ui) {
        use crate::extras::{settings_control_row_right,settings_segmented,settings_hint};
        ui.indent("wireless-resident-settings",|ui|{
            let mut style=self.settings.bluetooth_resident_style;
            settings_control_row_right(ui,"常驻样式",|ui|{settings_segmented(ui,&mut style,&[(0,"环 + 数字"),(1,"只显示环")],76.0);});
            self.settings.bluetooth_resident_style=style;
            settings_hint(ui,if style==0 {"小环加百分比，信息最完整"} else {"只显示小环；电量偏低或充电中的设备才显示百分比"});
            ui.add_space(6.0);
            let mut custom=self.settings.bluetooth_resident_custom;
            settings_control_row_right(ui,"常驻设备",|ui|{settings_segmented(ui,&mut custom,&[(false,"自动"),(true,"自定义")],62.0);});
            self.settings.bluetooth_resident_custom=custom;
            settings_hint(ui,if custom {"只在勾选的设备里挑；常驻条放不下时先显示电量低的"} else {"自动挑电量最低的设备常驻"});
            if custom {
                let devices=self.wireless_devices.clone();
                if devices.is_empty() {settings_hint(ui,"连接设备后可以在这里勾选");}
                for device in &devices {
                    let mut on=self.settings.bluetooth_pinned.contains(&device.id);
                    let label=match device.battery {Some(level)=>format!("{} · {level}%",device.name),None=>format!("{} · 未上报电量",device.name)};
                    if ui.checkbox(&mut on,label).changed() {
                        if on {self.settings.bluetooth_pinned.push(device.id.clone());} else {self.settings.bluetooth_pinned.retain(|id|id!=&device.id);}
                    }
                }
            }
        });
    }
}
