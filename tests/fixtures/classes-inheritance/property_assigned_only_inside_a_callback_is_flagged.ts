class Lazy {
    value: number;

    constructor() {
        const later = () => {
            this.value = 1;
        };
        later();
    }
}
