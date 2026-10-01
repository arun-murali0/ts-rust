// Expect: exactly ONE error, on the last `return x;` (string | null is not
// assignable to string), same as tsc.
// Gap if NO error is reported: the guard clause inside the arrow function
// narrowed `x` for the enclosing function after the callback.
function run(x: string | null): string {
    const check = () => {
        if (x === null) {
            return 0;
        }
        return 1;
    };
    check();
    return x;
}
