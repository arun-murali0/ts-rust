const obj = { "a": 1, "b": 2 };
// If the string-literal keys were dropped, obj would have no properties at
// all, and this access would be "property does not exist" rather than a
// type mismatch.
const x: string = obj.a;
