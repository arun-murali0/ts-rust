function f(xs: number[]): number {
    let s = 0;
    for (const x of xs) {
        const g = (): number => {
            if (x > 0) {
                return 1;
            }
            return 0;
        };
        s = s + g();
        if (s > 5) {
            break;
        }
    }
    return s;
}
