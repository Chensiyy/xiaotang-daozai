function refresh() {
    // 各时区使用全年固定偏移，城市设置可在列表与时区输入开放后接入。
    const cities = [["北京时间", 480], ["东京", 540], ["新德里", 330], ["协调世界时", 0]];
    return {status:{
        state:"idle",
        label:"世界时钟",
        clock:cities.map(([label, utc_offset_minutes]) => ({label, utc_offset_minutes, format:"HH:mm:ss"}))
    }};
}
