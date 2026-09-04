// Was a class declaration originally; classes are now checked as of V3c,
// so this no longer exercised the fallback path it's meant to test.
// A `for` loop is still genuinely unsupported (see
// bridge/statements.rs's stmt_kind_name), so it's a stable stand-in for
// "some statement kind ts-rust doesn't understand yet."
for (let i = 0; i < 10; i = i + 1) {
    i;
}
