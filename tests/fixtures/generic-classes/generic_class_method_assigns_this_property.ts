class Box<T> {
    value: T;
    constructor(value: T) {
        this.value = value;
    }

    replace(next: T): void {
        this.value = next;
    }
}

const box = new Box<number>(1);
box.replace("oops");
