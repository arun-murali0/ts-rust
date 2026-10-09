function f(read: () => string | null): number {
  let line: string | null;
  if ((line = read()) !== null) {
    return line.length;
  }
  return 0;
}
