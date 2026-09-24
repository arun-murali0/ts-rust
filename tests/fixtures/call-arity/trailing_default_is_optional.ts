function greet(name: string, greeting: string = "hello"): string {
    return greeting + name;
}

// A trailing default can be left out, so neither call is an arity error.
greet("world");
greet("world", "hi");
