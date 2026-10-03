function f(xs: number[]): number {
    let s = 0;
    for (const x of xs) {
        continue;
        s = s + x;
    }
    return s;
}
