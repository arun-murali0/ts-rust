interface Box<T> {
    value: T;
}

function label(b: Box<number>): string {
    return b;
}
