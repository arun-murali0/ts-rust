function f(x: string | null, again: boolean): number {
    while (again) {
        again = false;
    }
    if (x !== null) {
        const s: string = x;
        return 1;
    }
    return 0;
}
