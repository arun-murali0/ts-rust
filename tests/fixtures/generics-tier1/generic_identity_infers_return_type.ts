function identity<T>(value: T): T {
    return value;
}

const n: number = identity(5);
const s: string = identity("hello");
