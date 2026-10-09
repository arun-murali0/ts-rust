function total(xs: number[]): number {
  let sum = 0;
  for (const x of xs) {
    sum = sum + x;
  }
  return sum;
}
