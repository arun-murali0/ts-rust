function f(kind: "a" | "b" | "c", x: string | null): number {
  switch (kind) {
    case "a":
      if (x === null) {
        return 0;
      }
    case "b":
      return x.length;
    default:
      return 0;
  }
}
