function identity<T>(value: T): T {
    return value;
}

const wrong: string = identity(5);
