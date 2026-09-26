class Box<T> {
    value: T;
    constructor(value: T) {
        this.value = value;
    }
}

const b: Box<number, string> = new Box(1);
