function loopCheck(isDone: () => boolean): number {
    while (isDone()) {
        const bad: string = 5;
    }
    return 0;
}
