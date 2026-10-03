function f(x: number): number {
    let r = 0;
    try {
        if (x > 0) {
            r = 1;
        }
    } catch (e) {
        r = 2;
    } finally {
        r = r + 1;
    }
    return r;
}
