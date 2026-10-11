function call(f: ((a: string, b: number) => string) & ((a: number, b: string) => number)): void {
    f(1, 2);
}
