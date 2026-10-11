interface Handlers {
    run: ((x: string) => string) & ((x: number) => number);
}

function go(h: Handlers): void {
    h.run(true);
}
