function f(): number {
  return 1;
  // Three statements in the same dead region: reported once, at the first
  // one, not three times. Since the first one already stops the walk, the
  // other two are never even visited.
  return 2;
  return 3;
  return 4;
}
