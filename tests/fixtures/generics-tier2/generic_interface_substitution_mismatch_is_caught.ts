interface Box<T> {
    value: T;
}

// Box<string> requires value: string, so assigning a number-holding object
// literal to it must be reported.
const box: Box<string> = { value: 5 };
