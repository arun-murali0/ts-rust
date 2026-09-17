interface HasLength {
    length: number;
}

interface Box {
    length: number;
    extra: string;
}

function logLength<T extends HasLength>(value: T): number {
    return value.length;
}

function makeBox(): Box {
    return { length: 5, extra: "x" };
}

const result: number = logLength(makeBox());
