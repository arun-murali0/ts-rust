class Counter {
    count: number = 0;

    scheduleIncrement(): undefined {

        const direct: number = this.count;

        const callback = function () {
            const bad: string = this.count;
        };
        callback();
    }
}
