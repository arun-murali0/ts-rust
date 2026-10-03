function f(): number {
    let i = 0;
    while (true) {
        switch (i) {
            case 3:
                return i;
            default:
                i = i + 1;
        }
    }
}
