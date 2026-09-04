interface A {
    b: B;
}

interface B {
    value: number;
}

const a: A = { b: { value: 1 } };
