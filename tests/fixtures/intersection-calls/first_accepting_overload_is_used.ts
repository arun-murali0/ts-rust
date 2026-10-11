function call(f: ((x: string) => string) & ((x: number) => number)): string {
    return f("a");
}
