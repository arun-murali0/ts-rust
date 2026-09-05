// Regression fixture for the narrower fix on top of
// one_unresolvable_member_makes_whole_class_unsupported.ts: that fixture
// is right that a method with no return type annotation at all leaves
// nothing sound to record, so the whole class still fails there.
//
// This is a different, narrower gap: getValue's return type IS known
// (number) — only its parameter isn't annotated. Failing the entire
// class over that would mean losing all checking on `count` too, purely
// because of one loosely-typed method elsewhere in the same class. This
// is now registered with `is_untyped: true` instead (the same mechanism
// declare.rs already uses for a class whose *constructor* has an
// untyped parameter — see untyped_constructor_param_skips_arity_check.ts),
// so `count` and every other correctly-typed member stay checkable.
class Widget {
  count: number;

  getValue(value): number {
    return 1;
  }
}

const w: Widget = new Widget();

// Proves the class actually resolved: `count` is a real, typed
// property, not a symptom of the whole class collapsing to Error.
const badCount: string = w.count;

// Proves check_callable's is_untyped branch still checks each argument
// expression on its own terms even though arity/param-type comparison
// is skipped: the `1 - "x"` mismatch inside the call should still be
// caught, alongside a warning that arity itself wasn't checked. (Not
// `1 + "x"`: `+` deliberately allows number/string concatenation,
// matching real TS/JS semantics — see infer_binary_expression_type's
// Addition arm. `-` has no such string case, so it's the one that
// actually exercises "argument still checked" here.)
w.getValue(1 - "x");
