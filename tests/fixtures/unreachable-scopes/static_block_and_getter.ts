class A {
    get v(): number {
        if (this.w > 0) {
            return 1;
        }
        return 2;
    }
    w: number = 0;
}
