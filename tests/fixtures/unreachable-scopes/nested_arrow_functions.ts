const outer = (a: number): ((b: number) => number) => {
    if (a > 0) {
        return (b: number) => {
            if (b > 0) {
                return 1;
            }
            return 2;
        };
    }
    return (b: number) => b;
};
