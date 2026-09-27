class Box<T> {
    value: T;
    constructor(value: T) {
        this.value = value;
    }
}

const box = new Box<string>(1);
