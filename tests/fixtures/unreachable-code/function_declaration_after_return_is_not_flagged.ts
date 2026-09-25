function outer(): number {
    return 1;

    // Verified, not assumed: oxc's builder places a hoisted declaration's node
    // in the function's entry block, not at its textual position, so this is
    // not marked unreachable even though nothing after `return 1;` runs.
    // Matches tsc's own TS7027, which exempts a function declaration for the
    // same reason (hoisting), even though the mechanism here is different: it
    // falls out of how oxc builds the graph, not a special case in this file.
    function neverCalled(): number {
        return 0;
    }
}
