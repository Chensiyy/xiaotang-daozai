//! Windows PnP 与 BLE 分开读取；慢 GATT 请求不能阻塞连接状态和系统电量。
use super::{device_kind,state::*};
use std::{sync::{Arc,atomic::{AtomicBool,Ordering},mpsc::{self,SyncSender}},time::{Duration,Instant}};
use windows::{core::{GUID,HSTRING,PCWSTR,RuntimeType},Foundation::{AsyncStatus,IAsyncOperation},Devices::{Bluetooth::{BluetoothCacheMode,BluetoothConnectionStatus,BluetoothDevice,BluetoothLEDevice,GenericAttributeProfile::{GattCommunicationStatus,GattDeviceService}},Enumeration::DeviceInformation},Storage::Streams::DataReader};
use windows::Win32::{Devices::{DeviceAndDriverInstallation::*,Properties::*},Foundation::HWND,System::WinRT::{RoInitialize,RoUninitialize,RO_INIT_MULTITHREADED}};

struct Apartment;
impl Apartment {fn new()->Option<Self>{unsafe {RoInitialize(RO_INIT_MULTITHREADED).ok()?;}Some(Self)}}
impl Drop for Apartment {fn drop(&mut self){unsafe {RoUninitialize();}}}

fn wait<T:RuntimeType+'static>(op:windows::core::Result<IAsyncOperation<T>>,stop:&AtomicBool)->Option<T> {
    let op=op.ok()?;let start=Instant::now();
    loop {
        if stop.load(Ordering::Relaxed)||start.elapsed()>Duration::from_secs(3){let _=op.Cancel();return None;}
        match op.Status().ok()? {
            AsyncStatus::Completed=>return op.GetResults().ok(),
            AsyncStatus::Started=>std::thread::sleep(Duration::from_millis(40)),
            _=>return None,
        }
    }
}

#[derive(Clone)]
struct PnpNode {address:String,container:Option<String>,battery:Option<u8>}
struct DeviceSet(HDEVINFO);
impl Drop for DeviceSet {fn drop(&mut self){unsafe {let _=SetupDiDestroyDeviceInfoList(self.0);}}}

fn property(set:HDEVINFO,data:&SP_DEVINFO_DATA,key:&DEVPROPKEY)->Option<(DEVPROPTYPE,Vec<u8>)> {
    let mut kind=DEVPROPTYPE::default();let mut bytes=[0u8;512];let mut size=0;
    unsafe {SetupDiGetDevicePropertyW(set,data,key,&mut kind,Some(&mut bytes),Some(&mut size),0).ok()?;}
    Some((kind,bytes.get(..size as usize)?.to_vec()))
}
fn pnp_nodes()->Option<Vec<PnpNode>> {
    const BATTERY:DEVPROPKEY=DEVPROPKEY{fmtid:GUID::from_u128(0x104ea319_6ee2_4701_bd47_8ddbf425bbe5),pid:2};
    let set=DeviceSet(unsafe {SetupDiGetClassDevsW(None,PCWSTR::null(),HWND::default(),DIGCF_PRESENT|DIGCF_ALLCLASSES).ok()?});
    let mut nodes=vec![];
    for i in 0..u32::MAX {
        let mut data=SP_DEVINFO_DATA{cbSize:std::mem::size_of::<SP_DEVINFO_DATA>() as u32,..Default::default()};
        if unsafe {SetupDiEnumDeviceInfo(set.0,i,&mut data)}.is_err(){break;}
        let mut text=[0u16;512];let mut size=0;
        if unsafe {SetupDiGetDeviceInstanceIdW(set.0,&data,Some(&mut text),Some(&mut size))}.is_err(){continue;}
        let instance=String::from_utf16_lossy(&text[..(size as usize).saturating_sub(1).min(text.len())]);
        let Some(address)=pnp_address(&instance) else {continue};
        let container=property(set.0,&data,&DEVPKEY_Device_ContainerId).and_then(|(ty,bytes)|{
            (ty==DEVPROP_TYPE_GUID&&bytes.len()==16&&bytes.iter().any(|b|*b!=0)).then(||bytes.iter().map(|b|format!("{b:02X}")).collect())
        });
        let battery=property(set.0,&data,&BATTERY).and_then(|(ty,bytes)|{
            (ty==DEVPROP_TYPE_BYTE&&bytes.len()==1).then(||bytes[0]).filter(|v|*v<=100)
        });
        nodes.push(PnpNode{address,container,battery});
    }
    Some(nodes)
}

fn discover(previous:&[Target],nodes:&[PnpNode],stop:&AtomicBool)->Option<Vec<Endpoint>> {
    let selectors=[(false,BluetoothDevice::GetDeviceSelectorFromConnectionStatus(BluetoothConnectionStatus::Connected).ok()?),
        (true,BluetoothLEDevice::GetDeviceSelectorFromConnectionStatus(BluetoothConnectionStatus::Connected).ok()?)];
    let mut found=vec![];
    for (ble,selector) in selectors {
        let selector=HSTRING::from(format!("({selector}) AND System.Devices.Aep.IsConnected:=System.StructuredQueryType.Boolean#True"));
        // 任何一个传输枚举失败，都不把不完整快照当作设备断开。
        let infos=wait(DeviceInformation::FindAllAsyncAqsFilter(&selector),stop)?;
        for info in infos {
            let id=info.Id().ok()?;let name=info.Name().ok()?.to_string();
            if name.trim().is_empty(){continue;}
            let native_id=id.to_string();
            let cached=previous.iter().find(|t|t.endpoint.id==native_id);
            let (address,kind)=if let Some(old)=cached {(old.endpoint.address.clone(),old.endpoint.kind)}else{
                let (address,class)=if ble {
                    if let Some(device)=wait(BluetoothLEDevice::FromIdAsync(&id),stop){
                        let result=(device.BluetoothAddress().ok(),device.Appearance().ok().and_then(|v|v.RawValue().ok()).map(u32::from));let _=device.Close();result
                    }else{(None,None)}
                }else if let Some(device)=wait(BluetoothDevice::FromIdAsync(&id),stop){
                    let result=(device.BluetoothAddress().ok(),device.ClassOfDevice().ok().and_then(|v|v.RawValue().ok()));let _=device.Close();result
                }else{(None,None)};
                let address=address.and_then(|a|normalize_address(&format!("{a:012X}"))).or_else(||normalize_address(native_id.rsplit('-').next().unwrap_or_default()));
                (address,device_kind(&name,!ble,class))
            };
            let container=address.as_ref().and_then(|a|nodes.iter().find(|n|&n.address==a&&n.container.is_some())).and_then(|n|n.container.clone());
            found.push(Endpoint{id:native_id,address,container,name,kind,ble});
        }
    }
    Some(found)
}

struct BleHandle(BluetoothLEDevice);
impl Drop for BleHandle {fn drop(&mut self){let _=self.0.Close();}}
struct GattService(GattDeviceService);
impl Drop for GattService {fn drop(&mut self){let _=self.0.Close();}}

fn read_gatt(target:&Target,stop:&AtomicBool)->Option<Sample> {
    let device=BleHandle(wait(BluetoothLEDevice::FromIdAsync(&HSTRING::from(&target.endpoint.id)),stop)?);
    if device.0.ConnectionStatus().ok()?!=BluetoothConnectionStatus::Connected {return None;}
    let services=wait(device.0.GetGattServicesForUuidWithCacheModeAsync(GUID::from_u128(0x0000180f_0000_1000_8000_00805f9b34fb),BluetoothCacheMode::Uncached),stop)?;
    if services.Status().ok()?!=GattCommunicationStatus::Success {return None;}
    let services:Vec<_>=services.Services().ok()?.into_iter().map(GattService).collect();
    let mut levels=vec![];
    for service in &services {
        let Some(chars)=wait(service.0.GetCharacteristicsForUuidAsync(GUID::from_u128(0x00002a19_0000_1000_8000_00805f9b34fb)),stop) else {continue};
        if chars.Status().ok()!=Some(GattCommunicationStatus::Success){continue;}
        for characteristic in chars.Characteristics().ok()? {
            let Some(result)=wait(characteristic.ReadValueWithCacheModeAsync(BluetoothCacheMode::Uncached),stop) else {continue};
            if result.Status().ok()!=Some(GattCommunicationStatus::Success){continue;}
            let buffer=result.Value().ok()?;
            if buffer.Length().ok()!=Some(1){continue;}
            let level=DataReader::FromBuffer(&buffer).ok()?.ReadByte().ok()?;
            if level<=100 {levels.push(level);}
        }
    }
    // 多个标准电池实例取最低值；没有描述证据时不臆测左右耳或充电盒。
    Some(Sample{key:target.key.clone(),session:target.session,reading:BatteryReading{percent:*levels.iter().min()?,source:BatterySource::Gatt,at:Instant::now(),charging:None}})
}

pub(super) fn run(tx:SyncSender<Vec<super::Device>>,stop:Arc<AtomicBool>) {
    let Some(_apartment)=Apartment::new() else {return};
    let (jobs,requests)=mpsc::sync_channel::<Vec<Target>>(1);
    let (results,readings)=mpsc::sync_channel::<Sample>(128);
    let gatt_stop=stop.clone();
    let worker=std::thread::Builder::new().name("daozai-bluetooth-gatt".into()).spawn(move||{
        let Some(_apartment)=Apartment::new() else {return};
        while !gatt_stop.load(Ordering::Relaxed){
            let Ok(mut targets)=requests.recv_timeout(Duration::from_millis(200)) else {continue};
            while let Ok(latest)=requests.try_recv(){targets=latest;}
            for target in targets.iter().filter(|t|t.endpoint.ble){
                if gatt_stop.load(Ordering::Relaxed){return;}
                if let Some(reading)=read_gatt(target,&gatt_stop){let _=results.try_send(reading);}
            }
        }
    }).ok();
    let mut state=State::default();let mut nodes=vec![];let mut battery_at=None;let mut last_targets=vec![];
    while !stop.load(Ordering::Relaxed){
        let now=Instant::now();
        let due=battery_at.is_none_or(|at:Instant|now.saturating_duration_since(at)>=Duration::from_secs(60));
        let mut fresh_nodes=if due {pnp_nodes()}else{None};
        if let Some(found)=&fresh_nodes {nodes=found.clone();}
        if let Some(found)=discover(&state.targets(),&nodes,&stop){
            state.discover(found,now);let targets=state.targets();
            let ids:Vec<_>=targets.iter().map(|t|(t.endpoint.id.clone(),t.session)).collect();
            let changed=ids!=last_targets;
            if due||changed {
                // 新连接需要重新查询，不能把上轮缓存重新打上当前时间。
                if changed&&!due {fresh_nodes=pnp_nodes();}
                if let Some(fresh)=fresh_nodes{
                    for target in &targets {
                        if let Some(level)=target.endpoint.address.as_ref().and_then(|a|fresh.iter().filter(|n|&n.address==a).filter_map(|n|n.battery).min()){
                            state.ingest(Sample{key:target.key.clone(),session:target.session,reading:BatteryReading{percent:level,source:BatterySource::Windows,at:Instant::now(),charging:None}});
                        }
                    }
                    nodes=fresh;
                }
                let _=jobs.try_send(targets);battery_at=Some(now);last_targets=ids;
            }
            while let Ok(reading)=readings.try_recv(){state.ingest(reading);}
            if matches!(tx.try_send(state.devices(Instant::now())),Err(mpsc::TrySendError::Disconnected(_))){break;}
        }
        for _ in 0..20 {if stop.load(Ordering::Relaxed){break;}std::thread::sleep(Duration::from_millis(100));}
    }
    stop.store(true,Ordering::Relaxed);drop(jobs);if let Some(worker)=worker{let _=worker.join();}
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore="按需读取本机已连接设备，等待两个来源返回"]
    fn bluetooth_live_sources(){
        let service=crate::bt::spawn();let start=std::time::Instant::now();let mut snapshots=0;
        while start.elapsed()<std::time::Duration::from_secs(20){
            if let Ok(devices)=service.rx.recv_timeout(std::time::Duration::from_secs(1)){
                snapshots+=1;
                if snapshots==1||start.elapsed().as_secs()>=18 {
                    for device in &devices {println!("{} | connected={} | battery={:?} | source={:?}",device.name,device.connected,device.battery,device.reading.as_ref().map(|r|r.source));}
                    println!("snapshot devices={}",devices.len());
                }
            }
        }
        assert!(snapshots>0,"Windows 设备枚举应返回完整快照");
    }
}
