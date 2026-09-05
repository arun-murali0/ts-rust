// Regression fixture: check_callable used to only exclude Type::Error
// from its "not callable" diagnostic, not Type::Any — meaning calling a
// value explicitly typed `any` incorrectly produced a false-positive
// "is not callable" error, even though calling an `any` is always legal
// in real TS/JS (that's the whole point of `any`: it erases checking).
let f: any = 5;
f(1, 2);

// Proves the fix didn't overcorrect into "skip checking Any calls
// entirely": each argument is still checked on its own terms even
// though the call itself can't be arity/type-checked. `1 - "x"` should
// still be flagged (Subtraction requires both operands to be number,
// unlike Addition's deliberate string-concatenation leniency).
f(1 - "x");
