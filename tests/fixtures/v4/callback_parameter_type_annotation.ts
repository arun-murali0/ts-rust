function apply(fn: (x: number) => number, value: number): number {
    return fn(value);
}

const doubled: number = apply((x: number) => x * 2, 5);
