// A spread that starts after every parameter has been filled has nowhere to land in a
// function with no rest parameter, so tsc still reports it (TS2556) on the spread.
function add(a: number, b: number): number {
  return a + b;
}

const rest: number[] = [3];

add(1, 2, ...rest);
