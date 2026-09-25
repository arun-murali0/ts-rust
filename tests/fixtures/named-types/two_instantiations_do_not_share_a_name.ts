interface Box<T> {
    value: T;
}

const numberBox: Box<number> = { value: 1 };
const stringBox: Box<string> = numberBox;
