const obj = { a: 1, b: 2 };
// obj's values are numbers -- this checks the key "a" survived at all
// (an unknown-property error, not a value-type mismatch).
const x: number = obj.a;
