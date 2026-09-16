function sumUpTo(check: (i: number) => boolean): number {
    for (let i: number = 0; check(i); ) {
        const ok: number = i;
    }
    return 0;
}
