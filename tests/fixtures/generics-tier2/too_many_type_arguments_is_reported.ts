interface Box<T> {
    value: T;
}

// Box declares one type parameter, so two type arguments are too many.
const b: Box<number, string> = { value: 1 };
