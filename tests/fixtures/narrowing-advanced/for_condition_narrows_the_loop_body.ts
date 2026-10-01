function run(x: string | null): number {
    let total = 0;
    for (let i = 0; x !== null && i < 3; i = i + 1) {
        const s: string = x;
        total = total + i;
    }
    return total;
}
