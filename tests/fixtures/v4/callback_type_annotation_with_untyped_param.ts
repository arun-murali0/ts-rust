function apply(fn: (x) => number, value: number): number {
    const result: string = fn(value);
    return 0;
}

const doubled: number = apply((x: number) => x * 2, 5);
