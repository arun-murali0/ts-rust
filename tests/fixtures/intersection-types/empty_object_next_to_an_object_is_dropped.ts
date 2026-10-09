interface A {
  a: number;
}

declare const x: A & {};
const y: A = x;
const z: A & {} = y;
