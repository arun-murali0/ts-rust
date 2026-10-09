function f(x: string | null): number {
  let n = 0;
  try {
    if (x === null) {
      return 0;
    }
    n = x.length;
  } finally {
    n = n + 1;
  }
  return n;
}
