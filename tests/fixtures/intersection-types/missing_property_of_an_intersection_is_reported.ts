interface A {
  a: number;
}
interface B {
  b: string;
}

const x: A & B = { a: 1 };
