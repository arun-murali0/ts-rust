function f(items: (string | null)[], more: () => boolean): number {
  let total = 0;
  let x: string | null = null;
  while (more()) {
    x = items[0] ?? null;
    if (x === null) {
      continue;
    }
    total = total + x.length;
  }
  return total;
}
