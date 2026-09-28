class Branchy {
    value: number;

    constructor(flag: boolean) {
        if (flag) {
            this.value = 1;
        } else {
            this.value = 2;
        }
    }
}
