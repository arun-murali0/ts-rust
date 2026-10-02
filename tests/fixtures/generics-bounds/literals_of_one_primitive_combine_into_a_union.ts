function pair<T extends number>(a: T, b: T): T {
    return a;
}

const r: 1 | 2 = pair(1, 2);
