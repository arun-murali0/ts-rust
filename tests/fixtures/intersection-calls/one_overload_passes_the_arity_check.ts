function call(f: ((x: string) => string) & ((x: number, y: number) => number)): void {
    f(true);
}
