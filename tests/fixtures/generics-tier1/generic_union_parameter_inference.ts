function unwrap<T>(value: T | undefined): T | undefined {
    return value;
}

const n: number | undefined = unwrap(5);

function both(mixed: number | string): number | string | undefined {
    return unwrap(mixed);
}

const bad: string = unwrap(5);
