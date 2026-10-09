interface A {
  a: number;
}
interface B {
  b: string;
}
interface C {
  c: boolean;
}
type AB = A & B;

declare const abc: AB & C;
const ab: AB = abc;
const c: C = abc;
const flat: A & B & C = abc;
