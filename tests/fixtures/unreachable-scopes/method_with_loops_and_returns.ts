class Q {
    items: number[] = [];
    find(x: number): number {
        for (const i of this.items) {
            if (i === x) {
                return i;
            }
        }
        return -1;
    }
    total(): number {
        let s = 0;
        for (const i of this.items) {
            if (i < 0) {
                continue;
            }
            s = s + i;
        }
        return s;
    }
}
