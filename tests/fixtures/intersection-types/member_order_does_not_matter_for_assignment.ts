interface A {
  a: number;
}
interface B {
  b: string;
}

declare const ab: A & B;
const ba: B & A = ab;
const again: A & B = ba;
