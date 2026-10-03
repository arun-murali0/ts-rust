function f(): number {
    return g();
    function g(): number {
        return 1;
    }
}
