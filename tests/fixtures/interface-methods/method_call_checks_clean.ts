interface Greeter {
    greet(name: string): string;
    shout(name: string, times?: number): string;
}

function run(g: Greeter): string {
    const first: string = g.greet("world");
    return g.shout(first, 2);
}
