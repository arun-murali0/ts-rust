// Regression fixture: a type annotation that fails to resolve (here, a
// name that was never declared at all, e.g. a typo) used to be silently
// treated exactly like "no annotation was written," registering the
// variable's inferred type with zero diagnostic anywhere. See
// bridge/statements.rs's AnnotationOutcome for the fix.
let x: DoesNotExist = 5;
