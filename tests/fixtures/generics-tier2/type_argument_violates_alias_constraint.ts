type Named<T extends { name: string }> = { item: T };

// { id: number } has no `name`, so it does not satisfy the constraint.
const bad: Named<{ id: number }> = { item: { id: 1 } };
