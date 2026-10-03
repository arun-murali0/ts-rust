// Expect exactly one error: in the true branch r has no `error`.
type Result = { ok: true; value: number } | { ok: false; error: string };

function f(r: Result): string {
    if (r.ok) {
        return r.error;
    }
    return "";
}
