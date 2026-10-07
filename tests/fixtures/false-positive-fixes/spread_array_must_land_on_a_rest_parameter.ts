function add(a: number, b: number): number {
  return a + b;
}

const pair: number[] = [1, 2];

// tsc cannot tell how many values an array holds, so it only accepts a spread of one where
// it lands on a rest parameter (or an optional one). Here it covers required parameters.
add(...pair);
add(1, ...pair);
