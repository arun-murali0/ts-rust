// The first type parameter is never used in the body, so it must still own
// position 0: Second<number, string> binds A = number and B = string.
interface Second<A, B> {
    value: B;
}

const ok: Second<number, string> = { value: "x" };
const bad: Second<number, string> = { value: 1 };
