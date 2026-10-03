function f(xs: number[]): number {
    let s = 0;
    for (let i = 0; i < xs.length; i = i + 1) {
        switch (xs[i]) {
            case 1:
                continue;
            case 2:
                s = s + 2;
                break;
            default:
                s = s + 1;
        }
        s = s + 10;
    }
    return s;
}
