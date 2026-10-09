function f(xs: (string | null)[]): number {
  let total = 0;
  for (const x of xs) {
    if (x !== null) {
      total = total + x.length;
    }
  }
  return total;
}
