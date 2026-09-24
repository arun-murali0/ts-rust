function scale(factor: number = 2, value: number): number {
  return factor * value;
}

// A default only makes a parameter optional when nothing required follows it.
// `factor` is followed by `value`, so both are required, as tsc counts them.
scale();
