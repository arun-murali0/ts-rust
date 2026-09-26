class Box<T> {
  value: T;
  constructor(value: T) {
    this.value = value;
  }

  set(value: T): void {
    this.value = value;
  }
}

const box = new Box<number>(1);
box.set("oops");
