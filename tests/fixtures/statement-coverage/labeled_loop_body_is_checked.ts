function run(): void {
  outer: for (const x of [1, 2]) {
    const s: string = x;
    continue outer;
  }
}
