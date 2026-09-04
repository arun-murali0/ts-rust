interface Config {
    a: number;
}

// Known gap, tracked in docs/ROADMAP.md: real tsc rejects this assignment
// because 'b' isn't a property of Config (the excess-property check for
// fresh object literals). ts-rust only implements plain width subtyping so
// far, which allows extra properties, so this currently passes.
const c: Config = { a: 1, b: 2 };
