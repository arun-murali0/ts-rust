interface A {
    a: number;
}

type H = A | Missing;

const h: H = null as any;
