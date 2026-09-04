// Regression fixture: declare.rs registers a class's constructor arity
// as 0 both when there's truly no constructor and when there IS one but
// a parameter is untyped (see bridge/expressions.rs's
// constructor_is_unresolvable for the full explanation). Without the
// workaround there, this would falsely report "Expected 0 argument(s),
// but got 2" on a perfectly reasonable call.
class Point {
    constructor(x, y) {}
}

const p = new Point(1, 2);
