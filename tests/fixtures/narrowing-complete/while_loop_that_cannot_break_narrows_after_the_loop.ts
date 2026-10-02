function drain(x: string | null): number {
    while (x !== null) {
        x = null;
    }
    const done: null = x;
    return 0;
}
