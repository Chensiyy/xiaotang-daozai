// 文本设置兼容当前宿主。设置类型开放后可改为 number、time、weekdays。
function parseTime(text) {
    if (!/^(?:[01][0-9]|2[0-3]):[0-5][0-9]$/.test(text)) return null;
    const [hour, minute] = text.split(":").map(Number);
    return hour * 60 + minute;
}

function localAt(day, minutes) {
    const at = new Date(day);
    at.setHours(0, minutes, 0, 0);
    return Math.floor(at.getTime() / 1000);
}

function dayNumber(date) {
    return date.getDay() || 7;
}

function refresh({settings, now}) {
    const local = new Date(now ?? Date.now());
    const seconds = Math.floor(local.getTime() / 1000);
    const salary = Number(settings.salary);
    const start = parseTime(settings.start);
    const end = parseTime(settings.end);
    const workdays = settings.days.split(",").map(Number);
    if (!Number.isFinite(salary) || salary < 0 || start === null || end === null || start >= end ||
        !workdays.every(day => Number.isInteger(day) && day >= 1 && day <= 7)) {
        return {status:{state:"idle", label:"今日工资", summary:"请检查月薪、上下班时间和工作日"}};
    }

    const startAt = localAt(local, start);
    const endAt = localAt(local, end);
    const day = new Date(local.getFullYear(), local.getMonth(), 1);
    let daysInMonth = 0;
    while (day.getMonth() === local.getMonth()) {
        if (workdays.includes(dayNumber(day))) daysInMonth++;
        day.setDate(day.getDate() + 1);
    }

    const isWorkday = workdays.includes(dayNumber(local));
    const afterStart = isWorkday && seconds >= startAt;
    const working = afterStart && seconds < endAt;
    let countdown;
    if (working) {
        countdown = {label:"距下班", target_at:endAt, format:"HH:mm:ss", done_label:"已下班"};
    } else {
        const next = new Date(local);
        while (!workdays.includes(dayNumber(next)) || localAt(next, start) <= seconds) {
            next.setDate(next.getDate() + 1);
        }
        countdown = {label:"距上班", target_at:localAt(next, start), format:"d天 HH:mm", done_label:"已上班"};
    }

    return {status:{
        state:working ? "running" : "idle",
        label:"今日工资",
        counter:{
            start_value:0,
            start_at:afterStart ? startAt : seconds,
            per_second:afterStart ? salary / daysInMonth / (endAt - startAt) : 0,
            until:afterStart ? endAt : undefined,
            decimals:2,
            prefix:"¥",
            sensitive:true
        },
        countdown
    }};
}
