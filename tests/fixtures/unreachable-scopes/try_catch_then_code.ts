function f(): number {
    let r = 0;
    try {
        r = 1;
    } catch (e) {
        r = 2;
    }
    return r;
}
