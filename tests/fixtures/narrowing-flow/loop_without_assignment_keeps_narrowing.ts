// Nothing in the body assigns x, so the narrowing from before the loop holds on every pass.
function f(go: boolean): number {
  let x: string | null = "a";
  let total = 0;
  while (go) {
    total = total + x.length;
  }
  return total;
}
