function f(flag: boolean): number {
    if (flag === true) {
        const t: true = flag;
        return 1;
    }
    const off: false = flag;
    return 0;
}
