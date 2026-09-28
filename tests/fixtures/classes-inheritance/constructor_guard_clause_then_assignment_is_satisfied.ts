class Guarded {
    value: number;

    constructor(flag: boolean) {
        if (!flag) {
            throw "no";
        }
        this.value = 1;
    }
}
