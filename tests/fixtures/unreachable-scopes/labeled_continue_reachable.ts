function f(): number {
    let r = 0;
    outer: for (let i = 0; i < 3; i = i + 1) {
        for (let j = 0; j < 3; j = j + 1) {
            if (j === 1) {
                continue outer;
            }
            r = r + 1;
        }
        r = r + 100;
    }
    return r;
}
