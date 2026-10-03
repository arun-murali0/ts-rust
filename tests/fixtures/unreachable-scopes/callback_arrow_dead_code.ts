function run(cb: () => number): number {
    return cb();
}
const r = run(() => {
    return 1;
    return 2;
});
