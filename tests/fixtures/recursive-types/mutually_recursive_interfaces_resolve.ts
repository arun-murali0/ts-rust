interface A {
    b: B | null;
}

interface B {
    a: A | null;
}

function unwrap(a: A): number {
    return a.b;
}
