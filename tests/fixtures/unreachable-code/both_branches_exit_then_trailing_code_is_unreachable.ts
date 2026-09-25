function f(x: number): number {
    if (x > 0) {
        return 1;
    } else {
        return 0;
    }
    // Neither branch falls through, so nothing reaches this.
    return 2;
}
