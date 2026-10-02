type Options = { id: number; label?: string } | { id: number; flag: boolean };

function pick(o: Options): number {
    if ("flag" in o) {
        const f: boolean = o.flag;
        return 1;
    }
    return o.id;
}
