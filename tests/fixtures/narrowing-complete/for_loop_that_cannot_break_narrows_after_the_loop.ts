function drain(x: string | null): number {
    for (; x !== null; ) {
        x = null;
    }
    const done: null = x;
    return 0;
}
