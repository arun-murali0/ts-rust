function f(x: number | null): number {
    if (x === null) {
        return 0;
    }
    // The ordinary case the leak fixes must not have broken: this still
    // narrows within the same function, just as before.
    return x;
}
