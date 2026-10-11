enum E {
    A,
    B,
}

function impossible(e: E & string): number {
    return e;
}
