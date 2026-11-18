//! 多来源快照：只按明确身份合并，保留读数来源、时间和连接代次。
use std::collections::BTreeSet;
use std::time::{Duration,Instant};

pub const FRESH_FOR:Duration=Duration::from_secs(180);

#[derive(Clone,Copy,Debug,Default,PartialEq,Eq)]
pub enum DeviceKind {Bone,Earbuds,Headphones,Speaker,Keyboard,Mouse,Gamepad,Phone,#[default] Other}

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum BatterySource {Windows,Gatt}
impl BatterySource {
    pub fn label(self)->&'static str {match self {Self::Windows=>"Windows 设备电量",Self::Gatt=>"蓝牙电池服务"}}
}

#[derive(Clone,Debug,PartialEq,Eq)]
pub struct BatteryReading {pub percent:u8,pub source:BatterySource,pub at:Instant}

#[derive(Clone,Debug,PartialEq,Eq)]
pub struct Device {
    pub id:String,
    pub name:String,
    pub battery:Option<u8>,
    pub kind:DeviceKind,
    pub reading:Option<BatteryReading>,
    pub connected:bool,
}
impl Default for Device {
    fn default()->Self {Self{id:String::new(),name:String::new(),battery:None,kind:DeviceKind::Other,reading:None,connected:true}}
}
impl Device {
    pub fn stale_at(&self,now:Instant)->bool {!self.connected||self.reading.as_ref().is_some_and(|r|now.saturating_duration_since(r.at)>FRESH_FOR)}
    pub fn summary(&self)->String {match self.battery.filter(|_|!self.stale_at(Instant::now())) {Some(b)=>format!("{} · 电量 {b}%",self.name),None=>self.name.clone()}}
}

#[derive(Clone,Debug)]
pub(super) struct Endpoint {
    pub id:String,pub address:Option<String>,pub container:Option<String>,pub name:String,pub kind:DeviceKind,pub ble:bool,
}
impl Endpoint {
    fn aliases(&self)->BTreeSet<String> {
        let mut aliases=BTreeSet::from([format!("endpoint:{}",self.id.to_ascii_uppercase())]);
        if let Some(address)=self.address.as_deref().and_then(normalize_address){aliases.insert(format!("mac:{address}"));}
        if let Some(container)=self.container.as_ref().filter(|v|!v.is_empty()){aliases.insert(format!("container:{}",container.to_ascii_uppercase()));}
        aliases
    }
}
#[derive(Clone,Debug)]
pub(super) struct Target {pub endpoint:Endpoint,pub key:String,pub session:u64}
#[derive(Clone,Debug)]
pub(super) struct Sample {pub key:String,pub session:u64,pub reading:BatteryReading}

struct Entry {
    key:String,aliases:BTreeSet<String>,name:String,kind:DeviceKind,connected:bool,seen:bool,last_seen:Instant,session:u64,readings:Vec<BatteryReading>,
}
#[derive(Default)]
pub(super) struct State {entries:Vec<Entry>,endpoints:Vec<Endpoint>,generation:u64}
impl State {
    pub fn discover(&mut self,mut endpoints:Vec<Endpoint>,now:Instant) {
        endpoints.sort_by(|a,b|a.id.cmp(&b.id));
        for entry in &mut self.entries {entry.seen=false;}
        for endpoint in &endpoints {
            let aliases=endpoint.aliases();
            let mut matches:Vec<_>=self.entries.iter().enumerate().filter(|(_,e)|!e.aliases.is_disjoint(&aliases)).map(|(i,_)|i).collect();
            if matches.is_empty(){
                self.generation+=1;
                let key=endpoint.address.as_deref().and_then(normalize_address).map(|v|format!("mac:{v}")).unwrap_or_else(||format!("endpoint:{}",endpoint.id.to_ascii_uppercase()));
                self.entries.push(Entry{key,aliases:aliases.clone(),name:endpoint.name.clone(),kind:endpoint.kind,connected:true,seen:true,last_seen:now,session:self.generation,readings:vec![]});
                continue;
            }
            let first=matches.remove(0);
            for index in matches.into_iter().rev(){
                let other=self.entries.remove(index);
                let entry=&mut self.entries[first];entry.aliases.extend(other.aliases);
                for reading in other.readings {merge_reading(&mut entry.readings,reading);}
            }
            let entry=&mut self.entries[first];
            if !entry.connected {self.generation+=1;entry.session=self.generation;entry.readings.clear();}
            entry.aliases.extend(aliases);entry.connected=true;entry.seen=true;entry.last_seen=now;
            if entry.kind==DeviceKind::Other {entry.kind=endpoint.kind;}
            if entry.name.is_empty(){entry.name=endpoint.name.clone();}
        }
        for entry in &mut self.entries {entry.connected=entry.seen;}
        self.entries.retain(|e|e.connected||now.saturating_duration_since(e.last_seen)<Duration::from_secs(600));
        self.endpoints=endpoints;
    }
    pub fn targets(&self)->Vec<Target> {
        self.endpoints.iter().filter_map(|endpoint|{
            let aliases=endpoint.aliases();let entry=self.entries.iter().find(|e|e.connected&&!e.aliases.is_disjoint(&aliases))?;
            Some(Target{endpoint:endpoint.clone(),key:entry.key.clone(),session:entry.session})
        }).collect()
    }
    pub fn ingest(&mut self,sample:Sample) {
        if sample.reading.percent>100 {return;}
        if let Some(entry)=self.entries.iter_mut().find(|e|e.key==sample.key&&e.session==sample.session&&e.connected){merge_reading(&mut entry.readings,sample.reading);}
    }
    pub fn devices(&self,now:Instant)->Vec<Device> {
        let mut devices:Vec<_>=self.entries.iter().map(|e|{
            let reading=e.readings.iter().max_by_key(|r|{
                let fresh=now.saturating_duration_since(r.at)<=FRESH_FOR;
                (fresh,fresh&&r.source==BatterySource::Gatt,r.at)
            }).cloned();
            Device{id:e.key.clone(),name:e.name.clone(),kind:e.kind,battery:reading.as_ref().map(|r|r.percent),reading,connected:e.connected}
        }).collect();
        devices.sort_by(|a,b|b.connected.cmp(&a.connected).then_with(||a.id.cmp(&b.id)));devices
    }
}
fn merge_reading(readings:&mut Vec<BatteryReading>,reading:BatteryReading) {
    if let Some(old)=readings.iter_mut().find(|r|r.source==reading.source){if reading.at>old.at {*old=reading;}}
    else {readings.push(reading);}
}
pub(super) fn normalize_address(value:&str)->Option<String> {
    let value:String=value.chars().filter(|c|!matches!(c,':'|'-')).collect();
    (value.len()==12&&value.bytes().all(|c|c.is_ascii_hexdigit())&&value!="000000000000").then(||value.to_ascii_uppercase())
}
pub(super) fn pnp_address(instance:&str)->Option<String> {
    let upper=instance.to_ascii_uppercase();
    if !upper.starts_with("BTH"){return None;}
    let tail=upper.split_once("DEV_")?.1;
    normalize_address(tail.split(['\\','&','_']).next()?)
}

#[cfg(test)]
#[path="state_tests.rs"]
mod tests;
