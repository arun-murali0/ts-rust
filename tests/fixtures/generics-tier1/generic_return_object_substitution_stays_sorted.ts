function wrap<T>(value: T): { second: T; first: string } {
    return { second: value, first: "a" };
}

const wrapped: { first: string; second: number } = wrap(5);
