interface Counter {
    next(): number;
}

function label(c: Counter): string {
    return c.next();
}
