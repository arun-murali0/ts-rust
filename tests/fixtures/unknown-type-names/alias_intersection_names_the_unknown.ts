interface A {
    a: number;
}

type G = A & Missing;

const g: G = null as any;
