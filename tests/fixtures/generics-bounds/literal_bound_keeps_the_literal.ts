function pick<T extends "a" | "b">(x: T): T {
    return x;
}

const r: "a" = pick("a");
