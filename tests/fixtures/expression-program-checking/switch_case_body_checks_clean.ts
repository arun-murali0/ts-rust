function describe(x: number): string {
    switch (x) {
        case 1: {
            const ok: number = 5;
        }
        default: {
            const alsoOk: number = 10;
        }
    }
    return "ok";
}
