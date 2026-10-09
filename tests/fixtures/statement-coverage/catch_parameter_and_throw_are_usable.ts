function run(): string {
  try {
    throw new Error("failed");
  } catch (e) {
    return e.message;
  } finally {
    const done: boolean = true;
  }
}
