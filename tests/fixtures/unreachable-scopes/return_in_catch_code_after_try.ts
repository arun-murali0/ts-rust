function f(): number {
    try {
        const a = 1;
    } catch (e) {
        return 2;
    }
    return 3;
}
