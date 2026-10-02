function one<T extends number>(x: T): T {
    return x;
}

const r: 1 = one(1);
