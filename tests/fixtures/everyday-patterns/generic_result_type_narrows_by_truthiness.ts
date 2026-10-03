type Result<T> = { ok: true; value: T } | { ok: false; error: string };

function unwrap<T>(r: Result<T>): T | undefined {
    if (r.ok) {
        return r.value;
    }
    return undefined;
}
