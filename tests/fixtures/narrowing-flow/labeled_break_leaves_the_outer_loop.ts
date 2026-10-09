function f(rows: (string | null)[], more: () => boolean): number {
  let found: string | null = null;
  outer: while (more()) {
    while (more()) {
      found = rows[0] ?? null;
      if (found !== null) {
        break outer;
      }
    }
  }
  return found.length;
}
