interface A {
  a: number;
}
interface B {
  b: string;
}

declare const x: A & B;
const c = x.c;
