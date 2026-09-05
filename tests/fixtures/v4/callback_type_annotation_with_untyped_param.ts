// Regression fixture: a TSFunctionType used as an annotation — here,
// apply's own `fn` parameter's type `(x) => number` — used to require
// EVERY one of its own params to be typed too, via the strict
// resolve_function_params. Since `x` has no annotation, that made the
// whole `(x) => number` annotation unresolvable, which in turn made
// `apply`'s own parameter list fail to resolve, which meant `apply`
// itself was never registered at all — silently: no diagnostic, `apply`
// simply doesn't exist as far as the checker is concerned, and Pass 2
// never even looks at its body (see statements.rs's FunctionDeclaration
// arm: no registered signature means the body isn't checked).
//
// Now TSFunctionType uses resolve_params_with_any_fallback, the same
// resolver an actual arrow-function value already used for this exact
// gap — `x` defaults to `any` instead of failing the annotation, so
// `apply` registers correctly and its body actually gets checked.
//
// The `const result: string = fn(value);` mismatch below is the actual
// proof: it can only be caught if `apply`'s body was checked at all,
// which depends entirely on `apply` having registered successfully.
function apply(fn: (x) => number, value: number): number {
    const result: string = fn(value);
    return 0;
}

const doubled: number = apply((x: number) => x * 2, 5);
