enum Flags {
    None = 0,
    Read = 1 << 0,
    Write = 1 << 1,
    ReadWrite = Read | Write,
    Next,
}

function mask(): number {
    return Flags.ReadWrite;
}

function next(): string {
    return Flags.Next;
}
