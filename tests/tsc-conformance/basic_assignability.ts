export {};

let count: number = "not a number";

function add(a: number, b: number): number {
  return a + b;
}

add(1, "two");

interface Point {
  x: number;
  y: number;
}

const origin: Point = { x: 0 };
