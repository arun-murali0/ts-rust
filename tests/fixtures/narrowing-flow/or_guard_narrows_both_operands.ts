function f(x: string | null | undefined): number {
  if (x === null || x === undefined) {
    return 0;
  }
  return x.length;
}
