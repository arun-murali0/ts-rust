interface Box<T> {
    get(): T;
    set(value: T): T;
}

function read(box: Box<number>): string {
    return box.get();
}
