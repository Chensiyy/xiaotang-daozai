//! 音乐播放器兼容状态：每个播放器的版本，以及进度、时长、跳转等每一项能不能用、为什么不能用。
//! 读取模块写入，设置页和岛上提示读取；也负责保存「自动适配」学到的读取位置。
use std::{collections::HashMap,sync::Mutex,time::Instant};

/// 播放器的一项能力。
#[derive(Clone,Copy,PartialEq,Eq,Hash,Debug)]
pub enum Item {
    /// 播放进度（歌词同步依赖它）。
    Progress,
    /// 歌曲时长。
    Duration,
    /// 拖动进度条跳转。
    Seek,
    /// 播放器本机数据整体（波点、酷我：歌名、进度、歌词都从这里来）。
    Reader,
}
impl Item {
    pub fn label(self)->&'static str {
        match self {Item::Progress=>"播放进度",Item::Duration=>"歌曲时长",Item::Seek=>"拖动跳转",Item::Reader=>"本机播放数据"}
    }
}

#[derive(Clone,PartialEq,Debug)]
pub enum Support {
    /// 已适配（版本在适配表里）。
    Ok,
    /// 不在适配表里，但已按数据特征自动找到。
    Learned,
    /// 正在按数据特征自动适配（需要播放几秒）。
    Learning,
    /// 不可用，附原因。
    Unavailable(String),
}
impl Support {
    pub fn usable(&self)->bool {matches!(self,Support::Ok|Support::Learned)}
}

#[derive(Clone,Debug)]
pub struct PlayerStatus {
    pub exe:&'static str,
    pub version:Option<String>,
    pub items:Vec<(Item,Support)>,
    pub updated:Instant,
}
impl PlayerStatus {
    pub fn name(&self)->&'static str {display_name(self.exe)}
    pub fn get(&self,item:Item)->Option<&Support> {self.items.iter().find(|(i,_)|*i==item).map(|(_,s)|s)}
}

pub fn display_name(exe:&str)->&'static str {
    match exe.to_ascii_lowercase().as_str() {
        "kugou.exe"=>"酷狗音乐","cloudmusic.exe"=>"网易云音乐","bodian_pc.exe"=>"波点音乐","kwmusic.exe"=>"酷我音乐",
        "qqmusic.exe"=>"QQ 音乐","sodamusic.exe"=>"汽水音乐","spotify.exe"=>"Spotify","applemusic.exe"=>"Apple Music",_=>"播放器",
    }
}

static PLAYERS:Mutex<Vec<PlayerStatus>>=Mutex::new(Vec::new());

fn with_player<T>(exe:&'static str,f:impl FnOnce(&mut PlayerStatus)->T)->T {
    let mut players=PLAYERS.lock().unwrap_or_else(|e|e.into_inner());
    let index=match players.iter().position(|p|p.exe==exe) {
        Some(index)=>index,
        None=>{players.push(PlayerStatus{exe,version:None,items:Vec::new(),updated:Instant::now()});players.len()-1}
    };
    let player=&mut players[index];player.updated=Instant::now();f(player)
}

pub fn set_version(exe:&'static str,version:&str) {with_player(exe,|p|p.version=Some(version.to_owned()));}

pub fn set(exe:&'static str,item:Item,support:Support) {
    with_player(exe,|p|match p.items.iter_mut().find(|(i,_)|*i==item) {
        Some((_,old))=>*old=support,
        None=>p.items.push((item,support)),
    });
}

/// 播放器退出或重启后，旧进程的结论作废。
pub fn clear(exe:&'static str,item:Item) {
    let mut players=PLAYERS.lock().unwrap_or_else(|e|e.into_inner());
    if let Some(player)=players.iter_mut().find(|p|p.exe==exe){player.items.retain(|(i,_)|*i!=item);}
}

pub fn get(exe:&str)->Option<PlayerStatus> {
    PLAYERS.lock().unwrap_or_else(|e|e.into_inner()).iter().find(|p|p.exe.eq_ignore_ascii_case(exe)).cloned()
}

/// 设置页显示用：最近更新的在前。
pub fn all()->Vec<PlayerStatus> {
    let mut players=PLAYERS.lock().unwrap_or_else(|e|e.into_inner()).clone();
    players.sort_by(|a,b|b.updated.cmp(&a.updated));players
}

/// 当前播放源的歌词同步是否受版本限制（进度或本机数据不可用）：岛上据此给出提示。
pub fn lyrics_limited(source:&str)->bool {
    let Some(exe)=super::music_log::player_exe(source) else{return false;};
    get(exe).is_some_and(|p|[Item::Progress,Item::Reader].iter().any(|item|matches!(p.get(*item),Some(Support::Unavailable(_)))))
}

// ---- 自动适配学到的读取位置：按组件文件名、PE 时间戳与映像大小区分版本，保存在配置目录 ----

fn store_path()->std::path::PathBuf {crate::model::config_dir().join("music-compat.json")}
fn key(module:&str,stamp:u32,size:u32)->String {format!("{}/{stamp:08x}/{size}",module.to_ascii_lowercase())}

static LEARNED:Mutex<Option<HashMap<String,usize>>>=Mutex::new(None);

fn load()->HashMap<String,usize> {
    if cfg!(test) {return HashMap::new();}
    let Ok(bytes)=std::fs::read(store_path()) else{return HashMap::new();};
    if bytes.len()>64*1024 {return HashMap::new();}
    serde_json::from_slice::<HashMap<String,usize>>(&bytes).unwrap_or_default()
}

fn save(map:&HashMap<String,usize>) {
    if cfg!(test) {return;}
    if let Ok(text)=serde_json::to_string_pretty(map) {
        let path=store_path();
        if let Some(parent)=path.parent(){let _=std::fs::create_dir_all(parent);}
        let _=std::fs::write(path,text);
    }
}

pub fn learned(module:&str,stamp:u32,size:u32)->Option<usize> {
    let mut store=LEARNED.lock().unwrap_or_else(|e|e.into_inner());
    store.get_or_insert_with(load).get(&key(module,stamp,size)).copied()
}

pub fn remember(module:&str,stamp:u32,size:u32,offset:usize) {
    let mut store=LEARNED.lock().unwrap_or_else(|e|e.into_inner());
    let map=store.get_or_insert_with(load);
    map.insert(key(module,stamp,size),offset);save(map);
}

pub fn forget(module:&str,stamp:u32,size:u32) {
    let mut store=LEARNED.lock().unwrap_or_else(|e|e.into_inner());
    let map=store.get_or_insert_with(load);
    if map.remove(&key(module,stamp,size)).is_some(){save(map);}
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn items_are_tracked_per_player_and_limit_lyrics_only_for_progress_or_reader(){
        set("KuGou.exe",Item::Seek,Support::Unavailable("接口不可用".into()));
        assert!(!lyrics_limited("kugou"),"跳转不可用不影响歌词");
        set("KuGou.exe",Item::Progress,Support::Learning);
        assert!(!lyrics_limited("kugou"),"自动适配中先不提示");
        set("KuGou.exe",Item::Progress,Support::Unavailable("版本不匹配".into()));
        assert!(lyrics_limited("kugou"));
        set("KuGou.exe",Item::Progress,Support::Learned);
        assert!(!lyrics_limited("kugou"));
        assert!(get("kugou.exe").unwrap().get(Item::Seek).is_some_and(|s|!s.usable()));
        clear("KuGou.exe",Item::Seek);
        assert!(get("KuGou.exe").unwrap().get(Item::Seek).is_none());
        assert!(!lyrics_limited("chrome.exe"));
    }
    #[test]
    fn learned_offsets_are_keyed_by_module_version(){
        remember("KGPlayer.dll",0x1234,4096,0x3949f8);
        assert_eq!(learned("kgplayer.dll",0x1234,4096),Some(0x3949f8));
        assert_eq!(learned("kgplayer.dll",0x1235,4096),None,"其他版本不能借用");
        forget("kgplayer.dll",0x1234,4096);
        assert_eq!(learned("kgplayer.dll",0x1234,4096),None);
    }
}
