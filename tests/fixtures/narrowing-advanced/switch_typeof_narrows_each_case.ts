function size(x: string | number | boolean): number {
    switch (typeof x) {
        case "string": {
            const s: string = x;
            return 1;
        }
        case "number":
        case "boolean": {
            const nb: number | boolean = x;
            return 2;
        }
        default:
            return 0;
    }
}
