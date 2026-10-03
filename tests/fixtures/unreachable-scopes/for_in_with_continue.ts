function f(o: { a: number }): number {
    let s = 0;
    for (const k in o) {
        if (k === "a") {
            continue;
        }
        s = s + 1;
    }
    return s;
}
