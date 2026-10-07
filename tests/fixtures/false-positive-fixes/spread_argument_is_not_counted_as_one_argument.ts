function sum(first: number, ...rest: number[]): number {
  return first + rest.length;
}

function count(...all: number[]): number {
  return all.length;
}

function add(a: number, b: number): number {
  return a + b;
}

function maybe(a?: number): number {
  return 1;
}

const pair: number[] = [1, 2];

// A spread stands for any number of arguments, so it is not counted as one. These are the
// placements tsc accepts for an array: onto a rest parameter, onto an optional one, and an
// array literal, which tsc reads as a tuple whose length it knows.
sum(1, ...pair);
count(...pair);
count(1, ...pair);
maybe(...pair);
add(...[1, 2]);
