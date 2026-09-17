interface HasLength {
    length: number;
}

interface NoLength {
    name: string;
}

function logLength<T extends HasLength>(value: T): number {
    return value.length;
}

function makeNoLength(): NoLength {
    return { name: "x" };
}

logLength(makeNoLength());
