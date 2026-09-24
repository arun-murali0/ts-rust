function greet(name: string, greeting: string = "hello"): string {
    return greeting + name;
}

// The default is omittable but `name` is not, so the range starts at 1.
greet();
