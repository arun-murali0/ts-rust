function area({ width, height }: { width: number; height: number }, scale: number): number {
    return width * height * scale;
}

// A destructured parameter has no single name, so the list of missing
// parameters is dropped rather than naming only some of them.
area();
