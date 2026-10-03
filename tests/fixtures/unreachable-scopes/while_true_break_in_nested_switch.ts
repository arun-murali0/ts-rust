function f(k: number): number {
    let i = 0;
    while (true) {
        switch (k) {
            case 1:
                i = i + 1;
                break;
            default:
                return i;
        }
        i = i + 1;
    }
}
