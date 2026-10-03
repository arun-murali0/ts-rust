function f(): number {
    let i = 0;
    do {
        i = i + 1;
        if (i > 2) {
            break;
        }
    } while (true);
    return i;
}
