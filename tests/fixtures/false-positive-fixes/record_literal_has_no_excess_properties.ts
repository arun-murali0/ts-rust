// A Record accepts any key. Its body is empty in this checker, so before the fix
// every key below was reported as an excess property.
const scores: Record<string, number> = { alice: 1, bob: 2 };
