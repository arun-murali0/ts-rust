function add(a: number, b: number): number {
  return a + b;
}

const pair: number[] = [1, 2];

// A spread stands for any number of arguments, so this is not an arity error.
add(...pair);
add(1, ...pair);
