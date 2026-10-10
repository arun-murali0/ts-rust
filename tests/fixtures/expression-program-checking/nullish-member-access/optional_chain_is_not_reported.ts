interface Inner {
    c: number;
}

interface Outer {
    b: Inner;
}

function viaChain(a: Outer | null): number | undefined {
    return a?.b.c;
}

function viaGuard(x: string | null): number {
    if (x === null) {
        return 0;
    }
    return x.length;
}
