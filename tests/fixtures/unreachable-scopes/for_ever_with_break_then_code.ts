function f(): number {
    let i = 0;
    for (;;) {
        i = i + 1;
        if (i > 3) {
            break;
        }
    }
    return i;
}
