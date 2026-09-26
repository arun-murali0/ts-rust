class Box<T> {
    value: T;
    constructor(value: T) {
        this.value = value;
    }
}

const numberBox: Box<number> = new Box(1);
const stringBox: Box<string> = numberBox;
