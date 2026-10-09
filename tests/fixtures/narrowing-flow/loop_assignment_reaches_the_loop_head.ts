// The assignment at the bottom of the body flows back to the top of the next pass, so at
// the read the variable is string | null, not the string it was first assigned.
function f(go: boolean): number {
  let x: string | null = "a";
  while (go) {
    const n = x.length;
    x = null;
  }
  return 0;
}
