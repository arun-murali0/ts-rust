interface Options {
    label?: string;
}

function describe(options: Options): string {
    const { label = "untitled" } = options;
    return label;
}
