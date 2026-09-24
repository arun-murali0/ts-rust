function identity<T>(value: T): T {
    return value;
}

const n: number = identity<number>(5);
const s: string = identity<string>("hello");
