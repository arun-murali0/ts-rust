interface Box<T extends string> {
    value: T;
}

// T must be a string, so Box<number> does not satisfy the constraint.
const bad: Box<number> = { value: 1 };
