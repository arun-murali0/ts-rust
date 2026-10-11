function call(f: ((x: string) => string) & ((x: number) => number)): void {
    f(true);
}
