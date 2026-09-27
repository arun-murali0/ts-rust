class Box<T> {
    value: T;
    constructor(value: T) {
        this.value = value;
    }
}

class NumberBox extends Box<number> {}

function label(box: NumberBox): string {
    return box.value;
}
