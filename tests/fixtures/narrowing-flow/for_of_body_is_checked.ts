function f(xs: (string | null)[]): number {
  let total = 0;
  for (const x of xs) {
    total = total + x.length;
  }
  return total;
}
