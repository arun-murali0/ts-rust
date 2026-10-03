class Box<T> {
    constructor(public value: T) {}
}

const b = new Box(1);
const n: number = b.value;
