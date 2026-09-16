function describe(x: number): string {
    switch (x) {
        case 1: {
            const bad: string = 5;
        }
    }
    return "ok";
}
