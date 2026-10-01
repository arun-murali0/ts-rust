function drain(x: string | null): number {
    let count = 0;
    while (x !== null) {
        const s: string = x;
        count = count + 1;
        x = null;
    }
    return count;
}
