function f(o: { a: number }): number {
    let s = 0;
    for (const k in o) {
        s = s + 1;
        break;
    }
    return s;
}
