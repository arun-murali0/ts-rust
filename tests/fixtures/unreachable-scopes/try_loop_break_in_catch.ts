function f(): number {
    let n = 0;
    for (;;) {
        try {
            n = n + 1;
            if (n > 3) {
                throw new Error("x");
            }
        } catch (e) {
            break;
        }
    }
    return n;
}
