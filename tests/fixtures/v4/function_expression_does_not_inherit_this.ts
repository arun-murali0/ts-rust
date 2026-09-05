// Regression fixture: infer_function_expression_type used to leave
// ctx.current_class_instance untouched, the same way infer_arrow_
// function_type correctly does for arrow functions (which DO lexically
// inherit `this` in real JS). But a plain `function` expression gets its
// own dynamic `this` binding, independent of any lexically enclosing
// class — so leaving current_class_instance set while checking a
// function expression's body incorrectly let `this` inside it resolve
// to the enclosing class's instance type.
class Counter {
    count: number = 0;

    scheduleIncrement() {
        // Sanity check: `this` in the method's own body (not inside a
        // nested function) should still correctly resolve to Counter.
        const direct: number = this.count;

        // The actual regression case. Before the fix, `this.count` here
        // incorrectly resolved to `number` (leaked in from the enclosing
        // class), so assigning it to a `string`-typed const produced a
        // real, false-positive "not assignable" diagnostic — proving the
        // bug existed. After the fix, `this` resolves to Error here
        // instead (unknown, not guessed), and Error is compatible with
        // any type, so this line now correctly produces NO diagnostic.
        const callback = function () {
            const bad: string = this.count;
        };
        callback();
    }
}
