class Box<T> {
    value: T;
    constructor(value: T) {
        this.value = value;
    }
}

function label(box: Box<number>): string {
    return box.value;
}
