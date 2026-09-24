interface Box<T> {
    get(): T;
    set(value: T): T;
}

function write(box: Box<number>): number {
    return box.set("text");
}
