class Box<T> {
    value: T;
    constructor(value: T) {
        this.value = value;
    }
    set(next: T): void {
        this.value = next;
    }
}

function write(box: Box<number>): void {
    box.set("text");
}
