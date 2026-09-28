class OneSided {
    value: number;

    constructor(flag: boolean) {
        if (flag) {
            this.value = 1;
        } else {
            console_log();
        }
    }
}

function console_log(): void {}
