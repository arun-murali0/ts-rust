interface Handler<T> {
    handle: (item: T, count: number) => number;
}

const handler: Handler<string> = {
    handle: (item: string, count: number) => count,
};

// Substituting T = string rebuilds the signature; the names must survive it.
handler.handle("x");
