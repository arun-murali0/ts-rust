function loopCheck(isDone: () => boolean): number {
    while (isDone()) {
        const ok: number = 5;
    }
    return 0;
}
