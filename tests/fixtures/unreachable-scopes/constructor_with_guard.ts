class A {
    v: number;
    constructor(x: number) {
        if (x < 0) {
            throw new Error("neg");
        }
        this.v = x;
    }
}
