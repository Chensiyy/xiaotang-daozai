function record(input, add, note) {
    const previous = input.storage?.day === input.today ? input.storage : null;
    if (note === undefined) note = previous?.note ?? "";
    const cups = (previous?.cups ?? 0) + add;
    return {
        status: {
            state: cups >= 8 ? "succeeded" : "running",
            label: cups + " / 8 杯",
            progress_pct: Math.min(100, cups / 8 * 100),
            fields: [{key: "note", label: "备注", value: note, max: 80}],
            actions: [{id: "add", label: "+1 杯"}]
        },
        storage: {day: input.today, cups, note},
        notice: cups === 8 && add === 1
            ? {id: "water-" + input.today, title: "喝水目标完成", body: "今天已记录 8 杯水。"}
            : null
    };
}

function refresh(input) {
    return record(input, 0);
}

function action(input) {
    return record(input, input.action.id === "add" ? 1 : 0, input.action.form.note);
}

function event(input) {
    return record(input, input.event.name === "focus.finished" ? 1 : 0);
}
