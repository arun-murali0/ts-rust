interface Box<T> {
    value: T;
}

const numberBox: Box<number> = { value: 1 };
const stringBox: Box<string> = { value: "hi" };

// A number-boxed value assigned where a string-boxed one is expected must
// still be caught: Box<number> and Box<string> are not interchangeable just
// because they come from the same generic interface.
const mismatched: Box<string> = numberBox;
