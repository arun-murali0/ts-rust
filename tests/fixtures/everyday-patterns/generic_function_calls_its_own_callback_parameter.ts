function map<T, U>(value: T, fn: (x: T) => U): U {
    return fn(value);
}

function twice<T>(value: T, fn: (x: T) => T): T {
    return fn(fn(value));
}

function compose<A, B, C>(f: (a: A) => B, g: (b: B) => C, a: A): C {
    return g(f(a));
}
