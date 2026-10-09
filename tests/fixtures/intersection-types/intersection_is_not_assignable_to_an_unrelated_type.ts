interface A {
  a: number;
}
interface B {
  b: string;
}

declare const ab: A & B;
const n: number = ab;
