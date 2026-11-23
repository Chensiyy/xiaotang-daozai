function refresh({ settings, today, tz }) {
    const day = 86400000;
    const current = Date.parse(today);
    const target = (year) => {
        const [, month, date] = settings.target.split("-").map(Number);
        const lastDay = new Date(Date.UTC(year, month, 0)).getUTCDate();
        const validDate = `${String(year).padStart(4, "0")}-${String(month).padStart(2, "0")}-${String(Math.min(date, lastDay)).padStart(2, "0")}`;
        return Date.parse(validDate);
    };
    let date = Date.parse(settings.target);
    if (settings.yearly === "true") {
        const year = Number(today.slice(0, 4));
        date = target(year);
        if (settings.mode === "since" && date > current) date = target(year - 1);
        if (settings.mode === "until" && date < current) date = target(year + 1);
    }
    const remaining = (date - current) / day;
    const card = remaining > 0 ? `距离${settings.title}还有 ${remaining} 天`
        : remaining < 0 ? `${settings.title}已过 ${-remaining} 天` : `今天是${settings.title}`;
    return {
        card,
        marker: remaining === 0 && settings.mark === "true" ? settings.title : "",
        next_refresh_at: (current + day) / 1000 - tz.offset_minutes * 60,
    };
}
