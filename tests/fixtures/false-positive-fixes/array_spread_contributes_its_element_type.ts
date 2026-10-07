const words: string[] = ["a"];

// The spread adds string, the literal adds number.
const mixed = [...words, 1];
const both: Array<string | number> = mixed;
