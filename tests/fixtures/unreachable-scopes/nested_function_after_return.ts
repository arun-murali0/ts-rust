function f(): number {
    function inner(): number {
        return 1;
    }
    return inner();
}
