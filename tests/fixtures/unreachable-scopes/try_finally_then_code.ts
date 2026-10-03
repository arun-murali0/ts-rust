function f(): number {
    let r = 0;
    try {
        r = 1;
    } finally {
        r = r + 1;
    }
    return r;
}
