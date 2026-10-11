function call(
    f: ((x: string) => string) & ((x: number) => number) & ((x: boolean) => boolean),
): void {
    f({});
}
