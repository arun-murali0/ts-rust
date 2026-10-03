type Result = { ok: true; value: number } | { ok: false; error: string };

function unwrap(r: Result): number {
    if (r.ok) {
        return r.value;
    }
    return 0;
}

function describe(r: Result): string {
    if (!r.ok) {
        return r.error;
    }
    return "fine";
}
