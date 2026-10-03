function f(xs: number[]): number {
    let s = 0;
    for (const x of xs) {
        if (x < 0) {
            continue;
        }
        s = s + x;
    }
    return s;
}
