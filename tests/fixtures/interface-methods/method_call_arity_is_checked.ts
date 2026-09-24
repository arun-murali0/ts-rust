interface Greeter {
    greet(name: string): string;
}

function run(g: Greeter): string {
    return g.greet();
}
