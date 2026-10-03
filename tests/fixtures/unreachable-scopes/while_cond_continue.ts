function f(n: number): number {
    let i = 0;
    while (i < n) {
        i = i + 1;
        if (i === 2) {
            continue;
        }
        i = i + 1;
    }
    return i;
}
