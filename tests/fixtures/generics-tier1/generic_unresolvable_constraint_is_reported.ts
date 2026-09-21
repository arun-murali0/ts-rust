interface Wrapper<T> {
    value: T;
}

function unwrap<T extends Wrapper<number>>(item: T): T {
    return item;
}
