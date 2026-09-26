interface Box<T> {
    value: T;
    next: Box<T> | null;
}

function chain(box: Box<number>): string {
    return box.value;
}
