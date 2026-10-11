function keep<T>(x: T & {}): T {
    return x;
}

const text: string & {} = null;
