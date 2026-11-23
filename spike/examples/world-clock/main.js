// 城市来自列表设置，时区偏移由岛仔按当前日期换算（含夏令时），时钟由岛仔逐秒走动。
function refresh({settings, zones}) {
    let cities = [];
    try {
        cities = JSON.parse(settings.cities);
    } catch {
        cities = [];
    }
    const clock = cities
        .filter(city => zones?.[city.zone])
        .slice(0, 4)
        .map(city => ({label: city.name, utc_offset_minutes: zones[city.zone].offset_minutes, format: "HH:mm:ss"}));
    if (clock.length === 0) {
        return {status: {state: "idle", label: "世界时钟", summary: "在设置里添加城市"}};
    }
    // 夏令时切换后偏移会变，每小时整点重新换算一次。
    const next = Math.ceil(Date.now() / 3600_000) * 3600;
    return {status: {state: "idle", label: "世界时钟", clock}, next_refresh_at: next};
}
