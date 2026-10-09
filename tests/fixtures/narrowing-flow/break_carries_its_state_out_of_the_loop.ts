// The only way out of the loop is the break, and at the break x is a string.
function f(ready: () => boolean): number {
  let x: string | null = null;
  while (true) {
    if (ready()) {
      x = "a";
      break;
    }
  }
  return x.length;
}
