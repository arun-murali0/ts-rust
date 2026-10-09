// Every exported declaration kind, used from inside the file. Before the export
// wrapper was looked through, none of these were declared and each was a warning.
export interface Point {
  x: number;
  y: number;
}

export type Id = number;

export enum Mode {
  Fast,
  Slow,
}

export class Counter {
  count: number;

  constructor(start: number) {
    this.count = start;
  }
}

export const origin: Point = { x: 0, y: 0 };

export function distance(a: Point, b: Point): number {
  return a.x - b.x + (a.y - b.y);
}

const id: Id = 1;
const mode: Mode = Mode.Fast;
const counter: Counter = new Counter(id);
const far: number = distance(origin, { x: 1, y: 1 });

export default function main(): number {
  return far + counter.count;
}
