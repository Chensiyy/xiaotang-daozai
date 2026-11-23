function refresh({settings, zones}) {
    const cities = JSON.parse(settings.cities);
    const clock = [];
    for (const city of cities) {
        const offset = zones[city.zone.trim()]?.offset_minutes;
        if (!Number.isInteger(offset)) continue;
        const name = city.label.replace(/[\x00-\x1f\x7f-\x9f]/g, "").trim() || city.zone.trim();
        clock.push({label:[...name].slice(0, 28).join(""), utc_offset_minutes:offset, format:"HH:mm:ss"});
    }
    const missing = cities.length - clock.length;
    const summary = !cities.length ? "请在插件设置中添加城市和时区"
        : missing ? `${missing} 个时区无法识别，请检查 IANA 时区名称` : undefined;
    return {status:{state:"idle", label:"世界时钟", summary, clock}};
}
