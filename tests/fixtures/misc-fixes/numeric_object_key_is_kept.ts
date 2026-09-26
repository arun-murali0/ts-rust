const obj = { 1: "one", 2: "two" };
// obj's values are strings, not numbers -- this checks the key "1" survived
// at all (an unknown-property error, not a value-type mismatch).
const x: string = obj["1"];
