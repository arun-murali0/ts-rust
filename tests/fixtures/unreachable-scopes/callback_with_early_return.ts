function run(cb: (x: number) => number): number {
    return cb(1);
}
const r = run((x) => {
    if (x > 0) {
        return 1;
    }
    return 2;
});
