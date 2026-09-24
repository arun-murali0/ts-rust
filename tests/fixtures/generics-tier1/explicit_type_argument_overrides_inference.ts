function wrap<T>(value: T): T[] {
    return [value];
}

// Without the explicit <string>, a bare wrap(5) would infer T = number and
// pass. The explicit type argument fixes T = string instead, so the number
// argument no longer matches T and this call must be reported.
const bad: string[] = wrap<string>(5);
