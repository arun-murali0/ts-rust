function f(a: boolean): number {
    const x = a ? 1 : 2;
    const y = a && x > 1;
    return y ? x : 0;
}
