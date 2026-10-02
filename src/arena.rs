use crate::fxhash::{FxHashMap, FxHasher};
use crate::types::{ObjectType, Type};
use smallvec::SmallVec;
use std::cell::RefCell;
use std::hash::{Hash, Hasher};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct TypeId(u32);

// The (left, right) pairs a recursive comparison is in the middle of. It is a
// SmallVec, not a Vec or a hash set: real comparisons are shallow, so the sixteen
// inline slots almost never spill to the heap and a lookup is a handful of integer
// compares, cheaper than hashing. A pathologically deep type still works, because
// the vector simply moves to the heap.
pub(crate) type PairStack = SmallVec<[(TypeId, TypeId); 16]>;

impl TypeId {
    // The slot number, for a caller that needs to put two ids in a fixed order, for
    // example to store a symmetric answer once instead of once per direction. TypeId is
    // deliberately not Ord: nothing should sort or rank types by where they sit in the
    // arena, so the ordering is opt-in and named for what it is.
    pub(crate) fn index(self) -> u32 {
        self.0
    }
}

// How many slots new() reserves for the primitives (number() .. void()). Those ids
// are shared by every use of the primitive and can never be renamed: see
// set_display_name.
const FIXED_SLOTS: u32 = 10;

// The 64-bit digest the intern tables are keyed on, in place of the content itself.
//
// Decision: key the tables on a digest and confirm every hit against the slot in
// `types`, which is the source of truth anyway.
// Why: keying on the content would store each interned composite twice (once in
// `types`, once as the map key) and clone it on every miss.
// Cost: a digest collision costs one missed reuse and can never merge two different
// types, because a hit is compared with the slot before it is trusted.
fn content_hash<T: Hash + ?Sized>(content: &T) -> u64 {
    let mut hasher = FxHasher::default();
    content.hash(&mut hasher);
    hasher.finish()
}

/// Counters for benchmarks and regression reports. They are snapshots of sizes the
/// arena already tracks, so asking for them does not walk the type graph.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TypeArenaStats {
    pub type_count: usize,
    pub capacity: usize,
    pub interned_types: usize,
    pub interned_unions: usize,
    pub named_types: usize,
    pub record_types: usize,
}

pub struct TypeArena {
    types: Vec<Type>,

    // How a TypeId should read in a diagnostic, when it's not just its
    // structural shape: an interface, class or alias by its declared name
    // ("Dog"), a generic instantiation with its arguments ("Box<number>"), an
    // enum by its name ("Weird"). Keyed by TypeId rather than carried on Type
    // itself, so display is a side concern display_type can consult, not
    // something every match arm over Type has to thread through. Safe only for a
    // TypeId that is unique to whatever it names: alloc() reuses one TypeId for
    // every identical anonymous composite (see its doc comment), so a name given
    // to such a shared id would show up on every other use of the same shape.
    // Anything that gets a name must therefore come from alloc_fresh,
    // alloc_object_placeholder or make_unique; set_display_name asserts this in
    // debug builds.
    display_names: FxHashMap<TypeId, String>,

    // The value type of a `Record<K, V>`-shaped TypeId (see
    // type_annotation::resolve_builtin_generic). Record has no real index
    // signature representation here -- this is a narrower, honest stand-in: a
    // property access on a Record-tagged TypeId returns this value type for
    // *any* key, rather than modelling K at all (so a Record<"a" | "b",
    // number> does not reject an unrelated key the way tsc would). Same
    // TypeId-uniqueness requirement as display_names above.
    record_value_types: FxHashMap<TypeId, TypeId>,

    // Auxiliary index behind alloc(): a digest of the content of every anonymous
    // composite allocated so far -> the one TypeId that holds it. `types` stays the
    // source of truth (a TypeId is still just an index into it); this map only answers
    // "does a slot for exactly this content already exist?" before a new one is
    // pushed. The digest comes from Type's shallow hash (see types.rs), so computing
    // it never walks the arena.
    //
    // A digest match is never trusted on its own: alloc() compares the slot it points
    // at with the incoming type. Two different types sharing a 64-bit digest is not
    // expected, but if it happens the second one is simply left out of the table and
    // gets a slot of its own. That costs one missed reuse and can never merge two
    // types that differ, which is the only failure that would matter.
    interned: FxHashMap<u64, TypeId>,

    // The same idea as `interned`, kept apart for unions. A union cannot go through
    // alloc(): its members must be flattened and deduplicated first, and that is
    // alloc_union's job, so the key here is the *finished* member list.
    //
    // Decision: the key is a digest of the members in the order alloc_union produced
    // them (checked against the slot on a hit, like `interned`), not of a sorted list.
    // Why: sorting would merge `A | B` with `B | A`, but the survivor would be
    // whichever was built first, so the member order printed in a diagnostic would
    // depend on build order instead of source order.
    // Cost: reordered unions get separate ids and miss the relation cache;
    // structurally_equal still treats them as one type. A repeated build of the same
    // union (what narrowing does on every condition) still hits.
    //
    // A miss is always safe: two unions that are equal by shape but sit at different
    // ids just get different keys and stay separate.
    interned_unions: FxHashMap<u64, TypeId>,

    // Answers to "does this type mention a type parameter" (see
    // semantic::generics::contains_type_param), one byte per TypeId: 0 not asked
    // yet, 1 no, 2 yes. Every call expression asks this about its callee's
    // parameter and return types, and nearly all of those are plain non-generic
    // types, so remembering the answer across calls turns those walks into a
    // single lookup. It sits behind a RefCell only because the question is asked
    // through &TypeArena; no borrow is held across a call.
    //
    // Cleared by set(), the one place a slot's content changes. A placeholder
    // that was still empty when it was scanned reads as "no parameter", and every
    // type built on top of it inherited that answer, so any set() drops them all.
    // New slots need no invalidation: an id nobody has asked about is just absent.
    param_scan: RefCell<Vec<u8>>,

    // Answers already given by structurally_equal_cached, one entry per unordered
    // pair (equality is symmetric, so (a, b) and (b, a) share a slot).
    //
    // Only alloc_union asks, and it asks the same questions again whenever narrowing
    // rebuilds a union from the same members. Every answer stays valid until a
    // placeholder is completed: set() is the one operation that changes what an
    // existing id means, so it empties this table, same as param_scan above.
    equality_cache: FxHashMap<(TypeId, TypeId), bool>,

    // Advances whenever an id that already exists changes meaning: set() filling in a
    // placeholder, or clear() starting over. Anything that remembers an answer about
    // a TypeId (the relation cache) compares generations instead of relying on each
    // caller to remember an invalidation rule. New ids do not advance it, since an id
    // nobody has asked about has no stale answer.
    generation: u64,
}

impl TypeArena {
    // The ten primitives are allocated in this fixed order so their ids are known
    // constants below (number(), string(), and so on). Any part of the checker that
    // needs "the number type" calls arena.number() directly instead of having to
    // thread a TypeId through from wherever that primitive was first resolved.
    pub fn new() -> Self {
        let mut arena = Self {
            types: Vec::new(),
            display_names: FxHashMap::default(),
            record_value_types: FxHashMap::default(),
            interned: FxHashMap::default(),
            interned_unions: FxHashMap::default(),
            param_scan: RefCell::new(Vec::new()),
            equality_cache: FxHashMap::default(),
            generation: 0,
        };

        arena.alloc(Type::Number);
        arena.alloc(Type::String);
        arena.alloc(Type::Boolean);
        arena.alloc(Type::Null);
        arena.alloc(Type::Undefined);
        arena.alloc(Type::Any);
        arena.alloc(Type::Unknown);
        arena.alloc(Type::Error);
        arena.alloc(Type::Never);
        arena.alloc(Type::Void);
        debug_assert_eq!(arena.types.len() as u32, FIXED_SLOTS);
        arena
    }

    // Allocates `ty`, reusing the existing TypeId when an identical anonymous
    // composite was allocated before (hash-consing). Objects, arrays, functions,
    // literals and generic parameters are reused; everything else always gets a
    // new slot:
    //   - the fixed primitives (numbered 0..=9 in new(), never allocated again),
    //   - unions, which are interned by alloc_union itself after it has flattened
    //     and deduplicated them (see interned_unions), and
    //   - object placeholders, which alloc_object_placeholder pushes directly.
    //
    // Because two calls can return the same TypeId, a TypeId does not identify one
    // allocation. Anything that needs an id that is uniquely its own
    // (to attach a display name or a Record value type to it) must use
    // alloc_fresh, or make_unique on an id it was handed.
    //
    // A GenericParameter is not at risk: its key includes its TypeParameterId,
    // so a `T` from one declaration never matches a `T` from another.
    pub fn alloc(&mut self, ty: Type) -> TypeId {
        if !Self::is_internable(&ty) {
            return self.push(ty);
        }
        let digest = content_hash(&ty);
        if let Some(&existing) = self.interned.get(&digest) {
            if self.types[existing.0 as usize] == ty {
                return existing;
            }
            // Same digest, different content: keep it out of the table (see the
            // note on `interned`) rather than displace the entry that is there.
            return self.push(ty);
        }
        let id = self.push(ty);
        self.interned.insert(digest, id);
        id
    }

    // Always pushes a new slot and never enters the intern table, so the id is
    // guaranteed to be unique to this call. For the few types that carry an
    // identity beyond their shape: an opaque `Promise<T>` or `Record<K, V>` (an
    // empty object distinguished only by a display name or a side-table entry),
    // and the named result of a generic instantiation.
    pub fn alloc_fresh(&mut self, ty: Type) -> TypeId {
        self.push(ty)
    }

    // Returns `id` itself when it is already unique to its holder, or a fresh copy
    // of its content when it is a shared intern-table id. Call this before naming
    // a type that came out of resolution (`type Scores = number[]`, an enum whose
    // one member collapsed to a literal): the resolved id may be the very same id
    // every other `number[]` uses, and naming it would rename all of them.
    pub fn make_unique(&mut self, id: TypeId) -> TypeId {
        if self.is_interned(id) {
            let copy = self.get(id).clone();
            self.push(copy)
        } else {
            id
        }
    }

    // A fresh, unshared copy of `id` that prints under the same name. The copy is
    // never entered in either intern table, so it can be renamed or completed later
    // without touching `id` or any other copy.
    //
    // Exists for the instantiation memo (see TypeNamespace::cache_instantiation).
    // Callers depend on every reference to `Box<number>` having a slot of its own:
    // `type Alias = Box<number>` renames the slot it is handed, so if the memo handed
    // out one shared slot an alias would rename every other `Box<number>` in the
    // file. So the memo keeps a private pristine slot and every hit gets a duplicate
    // of it.
    pub fn duplicate_named(&mut self, id: TypeId) -> TypeId {
        let copy = self.get(id).clone();
        let duplicate = self.push(copy);
        if let Some(name) = self.display_names.get(&id).cloned() {
            self.display_names.insert(duplicate, name);
        }
        duplicate
    }

    // Whether the way `id` prints in a diagnostic can no longer change. A generic
    // instantiation bakes its arguments' text into its own name once ("Box<Dog>"),
    // so it may only be reused while every argument reads the same way it did when
    // the name was made.
    //
    // The case this exists for: `interface Node { next: Box<Node> }`. While Node is
    // still resolving, its id is an empty, unnamed placeholder that prints as an
    // empty object. Reusing that spelling later, after Node has its name, would put
    // "Box<{}>" in a message that should say "Box<Node>".
    //
    // Deliberately narrow: a composite that is not named, an object or a function,
    // answers false, which only costs a missed reuse. Recursion is safe because only
    // an Object can close a cycle, and Object never recurses here.
    pub fn has_settled_display(&self, id: TypeId) -> bool {
        if self.display_names.contains_key(&id) {
            return true;
        }
        match self.get(id) {
            Type::Number
            | Type::String
            | Type::Boolean
            | Type::Null
            | Type::Undefined
            | Type::Any
            | Type::Unknown
            | Type::Error
            | Type::Never
            | Type::Void
            | Type::StringLiteral(_)
            | Type::NumberLiteral(_)
            | Type::BooleanLiteral(_)
            // A type parameter always prints as its own name, so it never changes.
            | Type::GenericParameter(..) => true,
            Type::Array(element) => self.has_settled_display(*element),
            Type::Union(members) => members.iter().all(|&m| self.has_settled_display(m)),
            Type::Object(_) | Type::Function(_) => false,
        }
    }

    fn push(&mut self, ty: Type) -> TypeId {
        self.types.push(ty);
        TypeId((self.types.len() - 1) as u32)
    }

    fn is_internable(ty: &Type) -> bool {
        matches!(
            ty,
            Type::Object(_)
                | Type::Array(_)
                | Type::Function(_)
                | Type::StringLiteral(_)
                | Type::NumberLiteral(_)
                | Type::BooleanLiteral(_)
                | Type::GenericParameter(..)
        )
    }

    // Whether `id` is the shared slot the intern table hands out for its content.
    // Checked by looking the content up rather than tracking a flag per slot: a
    // placeholder, an alloc_fresh id, or a slot rewritten by `set` can hold
    // content identical to an interned one, but the table maps that content to a
    // different id, so it correctly reports false.
    fn is_interned(&self, id: TypeId) -> bool {
        let ty = self.get(id);
        // A union must be answered from its own table. If this fell through to the
        // check below it would always say "not shared", make_unique would hand back
        // the shared id, and `type Status = "a" | "b"` would rename every identical
        // union in the file to "Status". Release builds would do it silently, since
        // the guards that would catch it are debug_assert.
        if let Type::Union(members) = ty {
            return self.interned_unions.get(&content_hash(members)) == Some(&id);
        }
        Self::is_internable(ty) && self.interned.get(&content_hash(ty)) == Some(&id)
    }

    // Flattens nested unions and drops never members, since never contributes
    // nothing to what a union can hold. A one-member result collapses to that
    // member directly rather than a redundant single-member Type::Union, so
    // callers get a plain type back instead of having to unwrap a trivial union.
    pub fn alloc_union(&mut self, members: Vec<TypeId>) -> TypeId {
        let mut flat: Vec<TypeId> = Vec::with_capacity(members.len());
        let mut queue = members;
        while let Some(id) = queue.pop() {
            match self.get(id) {
                Type::Union(nested) => {
                    queue.extend(nested.iter().copied());
                    continue;
                }
                Type::Never => continue,
                _ => {}
            }
            // Plain structurally_equal, not structurally_equal_cached. This loop is
            // the only place the cached form was tried, and a diagnostic-count
            // regression appeared there; the root cause was not established. The
            // uncached form is the one known to keep the counts stable, so it stays
            // until the cause is found. structurally_equal_cached is kept and tested
            // for reconnecting.
            let already_present = flat
                .iter()
                .any(|&existing| self.structurally_equal(existing, id));
            if !already_present {
                flat.push(id);
            }
        }

        match flat.len() {
            0 => self.never(),
            1 => flat[0],
            // Not alloc(): that table is keyed on a digest of a Type, and a union's
            // identity is its finished member list, which only exists here. Reusing
            // the id is what lets the subtype cache, keyed on (TypeId, TypeId), hit
            // when narrowing rebuilds the same union.
            _ => {
                let digest = content_hash(&flat);
                if let Some(&existing) = self.interned_unions.get(&digest) {
                    if matches!(&self.types[existing.0 as usize], Type::Union(m) if *m == flat) {
                        return existing;
                    }
                    return self.push(Type::Union(flat));
                }
                let id = self.push(Type::Union(flat));
                self.interned_unions.insert(digest, id);
                id
            }
        }
    }

    pub fn get(&self, id: TypeId) -> &Type {
        &self.types[id.0 as usize]
    }

    // Registers how type_id should print. A later call for the same TypeId
    // replaces the earlier name rather than erroring, since a generic's own
    // cached shape and a specific instantiation of it are sometimes the exact
    // same TypeId (see namespace::resolve's own comment on this) and the more
    // specific caller should win.
    //
    // A fixed primitive slot never takes a name. make_unique leaves those ids alone
    // (they are not internable), so `type Age = number` or an empty enum collapsing
    // to `never` would otherwise name the one slot every other `number` or `never`
    // shares, and display_type consults names before the structural match. tsc
    // prints such an alias as the primitive itself, so ignoring the name is also
    // what a message should say.
    pub fn set_display_name(&mut self, type_id: TypeId, name: impl Into<String>) {
        if type_id.0 < FIXED_SLOTS {
            return;
        }
        debug_assert!(
            !self.is_interned(type_id),
            "naming a shared (interned) TypeId would rename every identical type; \
             use alloc_fresh or make_unique first"
        );
        self.display_names.insert(type_id, name.into());
    }

    pub fn display_name(&self, type_id: TypeId) -> Option<&str> {
        self.display_names.get(&type_id).map(String::as_str)
    }

    pub fn set_record_value_type(&mut self, record_type: TypeId, value_type: TypeId) {
        debug_assert!(
            !self.is_interned(record_type),
            "a Record's value type must hang off a TypeId unique to it; use alloc_fresh"
        );
        self.record_value_types.insert(record_type, value_type);
    }

    pub fn record_value_type(&self, type_id: TypeId) -> Option<TypeId> {
        self.record_value_types.get(&type_id).copied()
    }

    // Allocates an empty object shape and hands back its id, to be filled in
    // later with `set` once the real properties are known. This is what lets
    // an interface, class, or type-literal alias refer to itself through a
    // property -- a linked list's `next: Node | null` -- before its own shape
    // is finished: the self-reference resolves to this placeholder's TypeId by
    // identity, and `set` completes that same id afterward (see
    // namespace::resolve for how it's used).
    //
    // Pushed directly, never through alloc(): an empty object would otherwise be
    // reused for every other empty object (and every other placeholder), and
    // `set` would then overwrite all of them at once. A placeholder also stays
    // out of the intern table after `set` completes it, because other types
    // already hold its raw TypeId and it could never be merged with an identical
    // shape retroactively.
    pub fn alloc_object_placeholder(&mut self) -> TypeId {
        self.push(Type::Object(ObjectType::new(Vec::new())))
    }

    // Overwrites whatever is already at id. Only meant for finishing a
    // placeholder from alloc_object_placeholder above: a property only ever
    // stores a TypeId, never a cloned Type, so nothing can be holding a stale
    // copy of the placeholder's old (empty) content by the time this runs.
    pub fn set(&mut self, id: TypeId, ty: Type) {
        debug_assert!(
            !self.is_interned(id),
            "set() would change the content of a shared (interned) slot and leave \
             the intern table pointing at the wrong type"
        );
        self.types[id.0 as usize] = ty;
        self.generation = self.generation.wrapping_add(1);
        self.param_scan.get_mut().clear();
        // Two unfinished placeholders compare equal (both are empty objects), and that
        // answer stops being true the moment either one is filled in.
        self.equality_cache.clear();
    }

    // The remembered answer for `id`, if contains_type_param has settled it.
    pub(crate) fn cached_mentions_type_param(&self, id: TypeId) -> Option<bool> {
        match self.param_scan.borrow().get(id.0 as usize) {
            Some(1) => Some(false),
            Some(2) => Some(true),
            _ => None,
        }
    }

    pub(crate) fn remember_mentions_type_param(&self, id: TypeId, found: bool) {
        let mut cache = self.param_scan.borrow_mut();
        let index = id.0 as usize;
        if cache.len() <= index {
            // Grow to the arena's current size in one step rather than one slot
            // at a time as new ids get asked about.
            let wanted = self.types.len().max(index + 1);
            cache.resize(wanted, 0);
        }
        cache[index] = if found { 2 } else { 1 };
    }

    // structurally_equal with a memo, for callers that already hold `&mut self` and ask
    // the same pair more than once.
    //
    // Each answer is computed by a fresh top-level structurally_equal, never taken from
    // inside a walk. Answers reached mid-walk rest on the assumption that a pair
    // already being compared is equal, which only holds for that one comparison, so
    // storing them would be wrong for the next caller.
    //
    // Not called from production code: alloc_union uses the plain structurally_equal
    // (see the note there). Kept and tested so it can be reconnected, so dead_code is
    // allowed outside test builds instead of deleting it.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn structurally_equal_cached(&mut self, a: TypeId, b: TypeId) -> bool {
        if a == b {
            return true;
        }
        let key = if a.0 <= b.0 { (a, b) } else { (b, a) };
        if let Some(&known) = self.equality_cache.get(&key) {
            return known;
        }
        let equal = self.structurally_equal(a, b);
        self.equality_cache.insert(key, equal);
        equal
    }

    // Whether two types are the same type by shape, not by arena slot.
    //
    // The derived PartialEq on Type cannot answer this: it compares a composite's
    // TypeId fields by raw slot, so `{ a: { b: number } }` written at two sites,
    // whose inner objects sit at different slots, compare as unequal even though
    // nothing distinguishes them. This follows TypeIds through the arena instead.
    //
    // Terminates even on recursive types. The graph is acyclic except through a
    // placeholder: alloc_object_placeholder hands out an id, the members are
    // resolved against it, and set() then fills it in, so `interface N { next: N |
    // null }` contains its own id. Comparing two such types (say two interfaces
    // with the same recursive shape) would otherwise go N -> M -> N -> M forever.
    // Only an Object can close a cycle, so an object pair already being compared
    // further up the stack is assumed equal (the coinductive rule, the same one
    // subtyping uses); any real difference elsewhere in the type still shows up.
    //
    // Identity is the one thing shape must not override. A GenericParameter is
    // equal only to the same declared parameter (its TypeParameterId), never to
    // another `T` that merely has the same name or bound.
    pub fn structurally_equal(&self, a: TypeId, b: TypeId) -> bool {
        self.structurally_equal_inner(a, b, &mut PairStack::new())
    }

    fn structurally_equal_inner(&self, a: TypeId, b: TypeId, seen: &mut PairStack) -> bool {
        if a == b {
            return true;
        }

        match (self.get(a), self.get(b)) {
            (Type::Number, Type::Number)
            | (Type::String, Type::String)
            | (Type::Boolean, Type::Boolean)
            | (Type::Null, Type::Null)
            | (Type::Undefined, Type::Undefined)
            | (Type::Any, Type::Any)
            | (Type::Unknown, Type::Unknown)
            | (Type::Error, Type::Error)
            | (Type::Never, Type::Never)
            | (Type::Void, Type::Void) => true,

            (Type::StringLiteral(x), Type::StringLiteral(y)) => x == y,
            // Bit pattern, the same rule Type's own Eq/Hash use, so an interned
            // literal and this comparison never disagree about 0.0 and -0.0.
            (Type::NumberLiteral(x), Type::NumberLiteral(y)) => x.to_bits() == y.to_bits(),
            (Type::BooleanLiteral(x), Type::BooleanLiteral(y)) => x == y,

            (Type::Array(x), Type::Array(y)) => self.structurally_equal_inner(*x, *y, seen),

            (Type::Function(f), Type::Function(g)) => {
                f.is_untyped == g.is_untyped
                    && f.params.len() == g.params.len()
                    && f.params.iter().zip(&g.params).all(|(p, q)| {
                        p.optional == q.optional
                            && p.rest == q.rest
                            && self.structurally_equal_inner(p.type_id, q.type_id, seen)
                    })
                    && self.structurally_equal_inner(f.return_type, g.return_type, seen)
            }

            // Pairwise, which is only right because ObjectType keeps its
            // properties sorted by name. is_method is part of the shape: it is in
            // the intern key, and subtyping treats a method bivariantly but a
            // function-valued property contravariantly, so merging the two in a
            // union would change what the union accepts.
            (Type::Object(x), Type::Object(y)) => {
                if x.properties.len() != y.properties.len() {
                    return false;
                }
                // Re-entering a pair already being compared is the cycle a
                // placeholder creates; see the note above the function.
                let pair = (a, b);
                if seen.contains(&pair) {
                    return true;
                }
                seen.push(pair);
                let equal = x.properties.iter().zip(y.properties.iter()).all(|(p, q)| {
                    p.name == q.name
                        && p.optional == q.optional
                        && p.is_method == q.is_method
                        && self.structurally_equal_inner(p.type_id, q.type_id, seen)
                });
                seen.pop();
                equal
            }

            // A union is a set: member order carries no meaning. Union members are
            // already deduplicated by alloc_union, so equal length plus every
            // member of one having a match in the other is set equality.
            (Type::Union(xs), Type::Union(ys)) => {
                xs.len() == ys.len()
                    && xs.iter().all(|&x| {
                        ys.iter()
                            .any(|&y| self.structurally_equal_inner(x, y, seen))
                    })
            }

            (Type::GenericParameter(x, _, _), Type::GenericParameter(y, _, _)) => x == y,

            _ => false,
        }
    }

    // Starts over for the next file in a session. Truncating instead of replacing the
    // vectors keeps their allocations, and the ten primitive slots survive untouched,
    // so number() and friends return the same ids as in a fresh arena. Every side
    // table is cleared with the type graph: a table that survived would let the next
    // file see a stale display name, a stale intern entry or a stale cached answer.
    // The generation moves forward so a relation cache built before the reset
    // discards its answers on its next use.
    pub fn clear(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.types.truncate(FIXED_SLOTS as usize);
        self.display_names.clear();
        self.record_value_types.clear();
        self.interned.clear();
        self.interned_unions.clear();
        self.param_scan.get_mut().clear();
        self.equality_cache.clear();
        debug_assert_eq!(self.types.len() as u32, FIXED_SLOTS);
    }

    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.types.len()
    }

    /// Sizes for benchmarks and heap-regression reports. Capacity is included on
    /// purpose: after a large file a reused arena keeps its allocation, so the type
    /// count alone understates what it holds. The maps report entry counts, not
    /// allocator capacity, which keeps this a semantic measure and not a detail of the
    /// hash table.
    pub fn stats(&self) -> TypeArenaStats {
        TypeArenaStats {
            type_count: self.types.len(),
            capacity: self.types.capacity(),
            interned_types: self.interned.len(),
            interned_unions: self.interned_unions.len(),
            named_types: self.display_names.len(),
            record_types: self.record_value_types.len(),
        }
    }

    // Fixed slots assigned in new(). These never change for the lifetime of an arena.
    pub fn number(&self) -> TypeId {
        TypeId(0)
    }
    pub fn string(&self) -> TypeId {
        TypeId(1)
    }
    pub fn boolean(&self) -> TypeId {
        TypeId(2)
    }
    pub fn null(&self) -> TypeId {
        TypeId(3)
    }
    pub fn undefined(&self) -> TypeId {
        TypeId(4)
    }
    pub fn any(&self) -> TypeId {
        TypeId(5)
    }
    pub fn unknown(&self) -> TypeId {
        TypeId(6)
    }

    pub fn error(&self) -> TypeId {
        TypeId(7)
    }

    pub fn never(&self) -> TypeId {
        TypeId(8)
    }

    pub fn void(&self) -> TypeId {
        TypeId(9)
    }
}

impl Default for TypeArena {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{FunctionType, ObjectType, Param, PropertyEntry, TypeParameterId};

    fn property(name: &str, type_id: TypeId, optional: bool) -> PropertyEntry {
        PropertyEntry {
            name: name.into(),
            type_id,
            optional,
            is_method: false,
        }
    }

    // Built with alloc_fresh, not alloc: these tests exercise structurally_equal on
    // identical shapes that sit in *different* slots, which alloc's hash-consing
    // would otherwise collapse into one before the comparison could matter. The
    // consing behavior itself is covered by the alloc_* tests further down.
    fn object(arena: &mut TypeArena, properties: Vec<PropertyEntry>) -> TypeId {
        arena.alloc_fresh(Type::Object(ObjectType::new(properties)))
    }

    // `{ <name>: <type_id> }` through the consing alloc(), for the tests that are
    // about alloc() itself rather than about structurally_equal.
    fn shared_object(arena: &mut TypeArena, name: &str, type_id: TypeId, optional: bool) -> TypeId {
        arena.alloc(Type::Object(ObjectType::new(vec![property(
            name, type_id, optional,
        )])))
    }

    // `{ a: { b: number } }`, built from scratch each call so every call lands in
    // fresh arena slots.
    fn nested(arena: &mut TypeArena) -> TypeId {
        let number = arena.number();
        let inner = object(arena, vec![property("b", number, false)]);
        object(arena, vec![property("a", inner, false)])
    }

    #[test]
    fn derived_equality_cannot_see_through_a_slot_but_structural_equality_can() {
        let mut arena = TypeArena::new();
        let first = nested(&mut arena);
        let second = nested(&mut arena);

        assert_ne!(first, second, "separately allocated, so different slots");
        assert_ne!(
            arena.get(first),
            arena.get(second),
            "this is the gap: the derived == compares the inner object's slot"
        );
        assert!(arena.structurally_equal(first, second));
    }

    #[test]
    fn union_collapses_structurally_identical_composites() {
        let mut arena = TypeArena::new();
        let first = nested(&mut arena);
        let second = nested(&mut arena);

        let union = arena.alloc_union(vec![first, second]);

        assert!(
            matches!(arena.get(union), Type::Object(_)),
            "two identical shapes must collapse to one member, not stay a two-member union"
        );
    }

    // The point of interning unions: a repeated build must give back the same id, or
    // the subtype cache keyed on (TypeId, TypeId) can never hit on it.
    #[test]
    fn building_the_same_union_twice_reuses_its_id() {
        let mut arena = TypeArena::new();
        let a = arena.alloc(Type::StringLiteral("a".to_string()));
        let b = arena.alloc(Type::StringLiteral("b".to_string()));

        let first = arena.alloc_union(vec![a, b]);
        let second = arena.alloc_union(vec![a, b]);

        assert_eq!(first, second);
    }

    #[test]
    fn unions_with_different_members_stay_separate() {
        let mut arena = TypeArena::new();
        let a = arena.alloc(Type::StringLiteral("a".to_string()));
        let b = arena.alloc(Type::StringLiteral("b".to_string()));
        let c = arena.alloc(Type::StringLiteral("c".to_string()));

        assert_ne!(arena.alloc_union(vec![a, b]), arena.alloc_union(vec![a, c]));
    }

    // Naming a shared union must not rename every other identical union. Alias and enum resolution call
    // make_unique before naming, so make_unique has to know a union can be shared.
    #[test]
    fn naming_a_union_through_make_unique_does_not_rename_the_shared_one() {
        let mut arena = TypeArena::new();
        let a = arena.alloc(Type::StringLiteral("a".to_string()));
        let b = arena.alloc(Type::StringLiteral("b".to_string()));
        let shared = arena.alloc_union(vec![a, b]);

        let own = arena.make_unique(shared);
        arena.set_display_name(own, "Status");

        assert_ne!(own, shared);
        assert_eq!(arena.display_name(own), Some("Status"));
        assert_eq!(arena.display_name(shared), None);
        // A later identical build still finds the shared one, not the named copy.
        assert_eq!(arena.alloc_union(vec![a, b]), shared);
    }

    // The property the instantiation memo depends on: renaming a duplicate must
    // leave its source alone, because an alias does exactly that to what it is given.
    #[test]
    fn a_named_duplicate_can_be_renamed_without_touching_its_source() {
        let mut arena = TypeArena::new();
        let number = arena.number();
        let source = arena.alloc_fresh(Type::Object(ObjectType::new(vec![PropertyEntry {
            name: "value".into(),
            type_id: number,
            optional: false,
            is_method: false,
        }])));
        arena.set_display_name(source, "Box<number>");

        let duplicate = arena.duplicate_named(source);
        arena.set_display_name(duplicate, "Alias");

        assert_ne!(duplicate, source);
        assert_eq!(arena.display_name(source), Some("Box<number>"));
        assert_eq!(arena.display_name(duplicate), Some("Alias"));
        assert!(arena.structurally_equal(source, duplicate));
    }

    #[test]
    fn an_unfinished_placeholder_does_not_have_a_settled_display() {
        let mut arena = TypeArena::new();
        let number = arena.number();
        let placeholder = arena.alloc_object_placeholder();

        assert!(arena.has_settled_display(number));
        assert!(!arena.has_settled_display(placeholder));

        // Naming it is what settles it, which is when resolution completes.
        arena.set_display_name(placeholder, "Node");
        assert!(arena.has_settled_display(placeholder));
    }

    #[test]
    fn settled_display_looks_through_arrays_and_unions() {
        let mut arena = TypeArena::new();
        let (number, string) = (arena.number(), arena.string());
        let placeholder = arena.alloc_object_placeholder();
        let settled_union = arena.alloc_union(vec![number, string]);
        let unsettled_union = arena.alloc_union(vec![number, placeholder]);
        let settled_array = arena.alloc(Type::Array(number));
        let unsettled_array = arena.alloc(Type::Array(placeholder));

        assert!(arena.has_settled_display(settled_union));
        assert!(!arena.has_settled_display(unsettled_union));
        assert!(arena.has_settled_display(settled_array));
        assert!(!arena.has_settled_display(unsettled_array));
    }

    // The digest is only a hint. If two different types ever share one, alloc() must
    // fall back to a slot of its own instead of returning the other type's id: that
    // would be a silent type confusion, the one failure this table must not have. A
    // real 64-bit collision cannot be produced on demand, so the table is seeded with
    // a wrong entry, which is exactly the state a collision would leave behind.
    #[test]
    fn a_digest_match_on_different_content_is_not_reused() {
        let mut arena = TypeArena::new();
        let (number, string) = (arena.number(), arena.string());
        let wanted = Type::Array(number);
        let unrelated = arena.alloc(Type::Array(string));
        arena.interned.insert(content_hash(&wanted), unrelated);

        let got = arena.alloc(wanted.clone());

        assert_ne!(got, unrelated);
        assert!(*arena.get(got) == wanted);
        assert!(
            !arena.is_interned(got),
            "a collided type must stay unshared"
        );
    }

    #[test]
    fn a_digest_match_on_different_union_members_is_not_reused() {
        let mut arena = TypeArena::new();
        let a = arena.alloc(Type::StringLiteral("a".to_string()));
        let b = arena.alloc(Type::StringLiteral("b".to_string()));
        let c = arena.alloc(Type::StringLiteral("c".to_string()));
        let unrelated = arena.alloc_union(vec![a, b]);
        // What alloc_union will build for [a, c], seeded to point at the wrong slot.
        let members = vec![c, a];
        arena
            .interned_unions
            .insert(content_hash(&members), unrelated);

        let got = arena.alloc_union(vec![a, c]);

        let again = arena.alloc_union(vec![a, c]);

        assert_ne!(got, unrelated);
        assert!(arena.structurally_equal(got, again));
    }

    #[test]
    fn a_repeated_equality_question_is_answered_from_the_memo_in_either_order() {
        let mut arena = TypeArena::new();
        let number = arena.number();
        let one = object(&mut arena, vec![property("x", number, false)]);
        let two = object(&mut arena, vec![property("x", number, false)]);

        assert!(arena.structurally_equal_cached(one, two));
        assert!(arena.structurally_equal_cached(two, one));

        assert_eq!(
            arena.equality_cache.len(),
            1,
            "(a, b) and (b, a) share one entry"
        );
    }

    #[test]
    fn completing_a_placeholder_drops_equality_answers_given_while_it_was_empty() {
        let mut arena = TypeArena::new();
        let number = arena.number();
        let a = arena.alloc_object_placeholder();
        let b = arena.alloc_object_placeholder();
        assert!(arena.structurally_equal_cached(a, b));

        arena.set(
            a,
            Type::Object(ObjectType::new(vec![property("x", number, false)])),
        );

        assert!(!arena.structurally_equal_cached(a, b));
    }

    #[test]
    fn union_keeps_composites_that_differ() {
        let mut arena = TypeArena::new();
        let (number, string) = (arena.number(), arena.string());
        let a = object(&mut arena, vec![property("a", number, false)]);
        let b = object(&mut arena, vec![property("a", string, false)]);

        let union = arena.alloc_union(vec![a, b]);

        assert!(matches!(arena.get(union), Type::Union(members) if members.len() == 2));
    }

    #[test]
    fn objects_that_differ_only_in_optionality_or_name_are_not_equal() {
        let mut arena = TypeArena::new();
        let number = arena.number();
        let required = object(&mut arena, vec![property("a", number, false)]);
        let optional = object(&mut arena, vec![property("a", number, true)]);
        let renamed = object(&mut arena, vec![property("b", number, false)]);

        assert!(!arena.structurally_equal(required, optional));
        assert!(!arena.structurally_equal(required, renamed));
    }

    #[test]
    fn object_property_order_at_construction_does_not_matter() {
        let mut arena = TypeArena::new();
        let (number, string) = (arena.number(), arena.string());
        let one = object(
            &mut arena,
            vec![property("z", string, false), property("a", number, false)],
        );
        let two = object(
            &mut arena,
            vec![property("a", number, false), property("z", string, false)],
        );

        assert!(arena.structurally_equal(one, two));
    }

    #[test]
    fn arrays_compare_by_element_shape() {
        let mut arena = TypeArena::new();
        let first_element = nested(&mut arena);
        let second_element = nested(&mut arena);
        let first = arena.alloc_fresh(Type::Array(first_element));
        let second = arena.alloc_fresh(Type::Array(second_element));
        let numbers = arena.alloc(Type::Array(arena.number()));

        assert!(arena.structurally_equal(first, second));
        assert!(!arena.structurally_equal(first, numbers));
    }

    #[test]
    fn functions_compare_by_parameters_and_return_type() {
        let mut arena = TypeArena::new();
        let (number, string) = (arena.number(), arena.string());
        let make = |arena: &mut TypeArena, param: TypeId, ret: TypeId, optional: bool| {
            arena.alloc_fresh(Type::Function(FunctionType {
                params: vec![Param {
                    type_id: param,
                    optional,
                    rest: false,
                    name: None,
                }],
                return_type: ret,
                is_untyped: false,
            }))
        };
        let base = make(&mut arena, number, string, false);
        let same = make(&mut arena, number, string, false);
        let other_param = make(&mut arena, string, string, false);
        let other_return = make(&mut arena, number, number, false);
        let optional_param = make(&mut arena, number, string, true);

        assert!(arena.structurally_equal(base, same));
        assert!(!arena.structurally_equal(base, other_param));
        assert!(!arena.structurally_equal(base, other_return));
        assert!(!arena.structurally_equal(base, optional_param));
    }

    #[test]
    fn union_equality_ignores_member_order() {
        let mut arena = TypeArena::new();
        let (number, string) = (arena.number(), arena.string());
        let one = arena.alloc_union(vec![number, string]);
        let two = arena.alloc_union(vec![string, number]);

        assert!(arena.structurally_equal(one, two));
    }

    #[test]
    fn generic_parameters_are_equal_only_by_declaration_identity() {
        let mut arena = TypeArena::new();
        let same_declaration = TypeParameterId::new(10, 0);
        let other_declaration = TypeParameterId::new(50, 0);

        let first = arena.alloc_fresh(Type::GenericParameter(same_declaration, "T".into(), None));
        let again = arena.alloc_fresh(Type::GenericParameter(same_declaration, "T".into(), None));
        let lookalike =
            arena.alloc_fresh(Type::GenericParameter(other_declaration, "T".into(), None));

        assert!(arena.structurally_equal(first, again));
        assert!(
            !arena.structurally_equal(first, lookalike),
            "another `T` with the same name is a different type parameter"
        );
    }

    #[test]
    fn object_type_new_sorts_its_properties() {
        let arena = TypeArena::new();
        let number = arena.number();
        let built = ObjectType::new(vec![
            property("c", number, false),
            property("a", number, false),
            property("b", number, false),
        ]);

        let names: Vec<&str> = built.properties.iter().map(|p| &*p.name).collect();
        assert_eq!(names, ["a", "b", "c"]);
    }

    // ---- hash-consing (alloc) ----

    #[test]
    fn alloc_reuses_the_id_for_identical_anonymous_composites() {
        let mut arena = TypeArena::new();
        let (number, string) = (arena.number(), arena.string());

        let array_a = arena.alloc(Type::Array(number));
        let array_b = arena.alloc(Type::Array(number));
        assert_eq!(array_a, array_b);

        let object_a = shared_object(&mut arena, "a", number, false);
        let object_b = shared_object(&mut arena, "a", number, false);
        assert_eq!(object_a, object_b);

        let literal_a = arena.alloc(Type::StringLiteral("x".into()));
        let literal_b = arena.alloc(Type::StringLiteral("x".into()));
        assert_eq!(literal_a, literal_b);

        let function = |arena: &mut TypeArena| {
            arena.alloc(Type::Function(FunctionType {
                params: vec![Param::required(number)],
                return_type: string,
                is_untyped: false,
            }))
        };
        assert_eq!(function(&mut arena), function(&mut arena));
    }

    #[test]
    fn alloc_keeps_different_content_apart() {
        let mut arena = TypeArena::new();
        let (number, string) = (arena.number(), arena.string());

        let numbers = arena.alloc(Type::Array(number));
        let strings = arena.alloc(Type::Array(string));
        assert_ne!(numbers, strings);

        let literal_a = arena.alloc(Type::StringLiteral("a".into()));
        let literal_b = arena.alloc(Type::StringLiteral("b".into()));
        assert_ne!(literal_a, literal_b);

        let yes = arena.alloc(Type::BooleanLiteral(true));
        let no = arena.alloc(Type::BooleanLiteral(false));
        assert_ne!(yes, no);

        let required = shared_object(&mut arena, "a", number, false);
        let optional = shared_object(&mut arena, "a", number, true);
        assert_ne!(required, optional);
    }

    #[test]
    fn object_property_order_does_not_stop_two_objects_sharing_an_id() {
        let mut arena = TypeArena::new();
        let (number, string) = (arena.number(), arena.string());
        let one = arena.alloc(Type::Object(ObjectType::new(vec![
            property("z", string, false),
            property("a", number, false),
        ])));
        let two = arena.alloc(Type::Object(ObjectType::new(vec![
            property("a", number, false),
            property("z", string, false),
        ])));

        assert_eq!(one, two, "ObjectType::new sorts, so the keys are identical");
    }

    #[test]
    fn number_literals_are_keyed_on_their_bits() {
        let mut arena = TypeArena::new();

        let positive_zero = arena.alloc(Type::NumberLiteral(0.0));
        let negative_zero = arena.alloc(Type::NumberLiteral(-0.0));
        assert_ne!(
            positive_zero, negative_zero,
            "0.0 and -0.0 have different bit patterns, so they are different keys"
        );
        assert!(
            !arena.structurally_equal(positive_zero, negative_zero),
            "structurally_equal must agree with the intern key about signed zero"
        );

        let nan_a = arena.alloc(Type::NumberLiteral(f64::NAN));
        let nan_b = arena.alloc(Type::NumberLiteral(f64::NAN));
        assert_eq!(
            nan_a, nan_b,
            "a NaN literal is reused, not allocated forever"
        );

        let one_a = arena.alloc(Type::NumberLiteral(1.0));
        let one_b = arena.alloc(Type::NumberLiteral(1.0));
        assert_eq!(one_a, one_b);
    }

    #[test]
    fn generic_parameters_collapse_only_within_one_declaration() {
        let mut arena = TypeArena::new();
        let declaration = TypeParameterId::new(10, 0);
        let other_declaration = TypeParameterId::new(50, 0);

        let first = arena.alloc(Type::GenericParameter(declaration, "T".into(), None));
        let again = arena.alloc(Type::GenericParameter(declaration, "T".into(), None));
        let lookalike = arena.alloc(Type::GenericParameter(other_declaration, "T".into(), None));

        assert_eq!(first, again);
        assert_ne!(
            first, lookalike,
            "another `T` with the same name stays distinct"
        );
    }

    #[test]
    fn parameter_names_keep_otherwise_identical_functions_apart() {
        let mut arena = TypeArena::new();
        let (number, string) = (arena.number(), arena.string());
        let named = |arena: &mut TypeArena, name: &str| {
            arena.alloc(Type::Function(FunctionType {
                params: vec![Param {
                    type_id: number,
                    optional: false,
                    rest: false,
                    name: Some(name.into()),
                }],
                return_type: string,
                is_untyped: false,
            }))
        };

        let x = named(&mut arena, "x");
        let y = named(&mut arena, "y");

        // A "missing argument" diagnostic names the parameter, so merging these
        // would let one function report the other's parameter name.
        assert_ne!(x, y);
        assert!(
            arena.structurally_equal(x, y),
            "still the same type by shape; only the ids differ"
        );
    }

    // Pins the limit of union interning: the key is order-sensitive, so a reordered
    // union is a separate id (see interned_unions for why the key is not sorted).
    // structurally_equal still treats the two as one type.
    #[test]
    fn reordered_unions_are_equal_by_shape_but_keep_separate_ids() {
        let mut arena = TypeArena::new();
        let (number, string) = (arena.number(), arena.string());

        let one = arena.alloc_union(vec![number, string]);
        let two = arena.alloc_union(vec![string, number]);

        assert_ne!(
            one, two,
            "order-sensitive key: reordering is a different id"
        );
        assert!(arena.structurally_equal(one, two));
    }

    #[test]
    fn clear_returns_to_the_primitive_baseline_and_keeps_capacity() {
        use crate::types::PropertyEntry;

        let mut arena = TypeArena::new();
        let number = arena.number();
        let object = arena.alloc_fresh(Type::Object(ObjectType::new(vec![PropertyEntry {
            name: "value".into(),
            type_id: number,
            optional: false,
            is_method: false,
        }])));
        arena.set_display_name(object, "Wide");
        let capacity_before = arena.stats().capacity;
        assert!(arena.len() > FIXED_SLOTS as usize);

        arena.clear();

        assert_eq!(arena.len(), FIXED_SLOTS as usize);
        assert!(arena.stats().capacity >= capacity_before);
        assert_eq!(arena.number(), TypeId(0));
        assert_eq!(arena.void(), TypeId(9));
        assert_eq!(arena.stats().named_types, 0);
        assert_eq!(arena.stats().interned_types, 0);
        assert_eq!(arena.display_name(object), None);

        let fresh = arena.alloc(Type::Array(arena.number()));
        assert_eq!(
            fresh,
            TypeId(FIXED_SLOTS),
            "ids restart right after the primitives"
        );
    }

    #[test]
    fn generation_moves_only_when_an_existing_id_changes_meaning() {
        let mut arena = TypeArena::new();
        let start = arena.generation();

        arena.alloc(Type::Array(arena.number()));
        assert_eq!(arena.generation(), start, "a new id has no stale answers");

        let placeholder = arena.alloc_object_placeholder();
        assert_eq!(arena.generation(), start);

        arena.set(placeholder, Type::Object(ObjectType::new(Vec::new())));
        assert_ne!(
            arena.generation(),
            start,
            "filling a placeholder changes an id"
        );

        let after_set = arena.generation();
        arena.clear();
        assert_ne!(
            arena.generation(),
            after_set,
            "clear() starts a new generation"
        );
    }

    #[test]
    fn stats_count_interned_and_named_types() {
        let mut arena = TypeArena::new();
        let before = arena.stats();
        let array = arena.alloc(Type::Array(arena.number()));
        let again = arena.alloc(Type::Array(arena.number()));
        assert_eq!(array, again);

        let after = arena.stats();
        assert_eq!(after.type_count, before.type_count + 1);
        assert_eq!(after.interned_types, before.interned_types + 1);
    }

    #[test]
    fn deep_recursive_comparison_spills_past_the_inline_stack() {
        use crate::types::PropertyEntry;

        // A chain deeper than PairStack's sixteen inline slots, so the recursion
        // guard has to move to the heap and still give the right answer.
        let mut arena = TypeArena::new();
        let mut left = arena.number();
        let mut right = arena.number();
        for _ in 0..40 {
            left = arena.alloc(Type::Object(ObjectType::new(vec![PropertyEntry {
                name: "next".into(),
                type_id: left,
                optional: false,
                is_method: false,
            }])));
            right = arena.alloc(Type::Object(ObjectType::new(vec![PropertyEntry {
                name: "next".into(),
                type_id: right,
                optional: false,
                is_method: false,
            }])));
        }
        assert!(arena.structurally_equal(left, right));
    }

    #[test]
    fn primitives_keep_their_fixed_slots() {
        let arena = TypeArena::new();
        assert_eq!(arena.number(), TypeId(0));
        assert_eq!(arena.void(), TypeId(9));
    }

    // ---- carve-outs: types that must keep an id of their own ----

    #[test]
    fn placeholders_are_never_shared_with_each_other_or_with_an_empty_object() {
        let mut arena = TypeArena::new();

        let first = arena.alloc_object_placeholder();
        let second = arena.alloc_object_placeholder();
        let empty = arena.alloc(Type::Object(ObjectType::new(Vec::new())));

        assert_ne!(first, second);
        assert_ne!(first, empty);
        assert_ne!(second, empty);
    }

    #[test]
    fn a_completed_placeholder_never_joins_the_intern_table() {
        let mut arena = TypeArena::new();
        let number = arena.number();
        let placeholder = arena.alloc_object_placeholder();
        arena.set(
            placeholder,
            Type::Object(ObjectType::new(vec![property("a", number, false)])),
        );

        let same_shape = shared_object(&mut arena, "a", number, false);

        assert_ne!(
            placeholder, same_shape,
            "other types already hold the placeholder's raw id, so it is never merged"
        );
        assert!(arena.structurally_equal(placeholder, same_shape));
    }

    #[test]
    fn two_open_placeholders_never_merge_through_a_shape_that_mentions_them() {
        let mut arena = TypeArena::new();
        let first = arena.alloc_object_placeholder();
        let second = arena.alloc_object_placeholder();

        // Both are still the same empty `{}` here. The key hashes the ids, not
        // what they point at, so these must stay two different arrays.
        let first_list = arena.alloc(Type::Array(first));
        let second_list = arena.alloc(Type::Array(second));

        assert_ne!(first_list, second_list);
    }

    #[test]
    fn alloc_fresh_never_reuses_an_id_even_for_identical_content() {
        let mut arena = TypeArena::new();
        let number = arena.number();

        let shared = arena.alloc(Type::Array(number));
        let fresh_a = arena.alloc_fresh(Type::Array(number));
        let fresh_b = arena.alloc_fresh(Type::Array(number));
        let after = arena.alloc(Type::Array(number));

        assert_ne!(fresh_a, shared);
        assert_ne!(fresh_a, fresh_b);
        assert_eq!(
            after, shared,
            "alloc_fresh does not disturb the intern table"
        );
    }

    #[test]
    fn make_unique_copies_a_shared_id_and_leaves_an_owned_one_alone() {
        let mut arena = TypeArena::new();
        let number = arena.number();

        let shared = arena.alloc(Type::Array(number));
        let copy = arena.make_unique(shared);
        assert_ne!(copy, shared);
        assert!(arena.structurally_equal(copy, shared));
        let again = arena.alloc(Type::Array(number));
        assert_eq!(again, shared, "the table still points at the original");

        let fresh = arena.alloc_fresh(Type::Array(number));
        assert_eq!(arena.make_unique(fresh), fresh);
        assert_eq!(arena.make_unique(arena.number()), arena.number());
    }

    #[test]
    fn naming_a_unique_copy_does_not_rename_the_shared_type() {
        let mut arena = TypeArena::new();
        let number = arena.number();

        let shared = arena.alloc(Type::Array(number));
        let scores = arena.make_unique(shared);
        arena.set_display_name(scores, "Scores");

        assert_eq!(arena.display_name(scores), Some("Scores"));
        assert_eq!(arena.display_name(shared), None);
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "interned")]
    fn naming_a_shared_id_is_caught_in_debug_builds() {
        let mut arena = TypeArena::new();
        let number = arena.number();
        let shared = arena.alloc(Type::Array(number));
        arena.set_display_name(shared, "Oops");
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "interned")]
    fn rewriting_a_shared_slot_with_set_is_caught_in_debug_builds() {
        let mut arena = TypeArena::new();
        let number = arena.number();
        let shared = arena.alloc(Type::Array(number));
        arena.set(shared, Type::Array(arena.string()));
    }

    // `interface N { next: N | null }` built the way namespace::resolve builds
    // it: placeholder first, members resolved against it, then set(). After set()
    // the type refers to its own id, so the arena is no longer acyclic.
    fn recursive_node(arena: &mut TypeArena, tail: TypeId) -> TypeId {
        let placeholder = arena.alloc_object_placeholder();
        let next = arena.alloc_union(vec![placeholder, tail]);
        arena.set(
            placeholder,
            Type::Object(ObjectType::new(vec![property("next", next, false)])),
        );
        placeholder
    }

    // Comparing two identically shaped recursive types goes a -> b -> a -> b ... and
    // only terminates because a pair already on the stack is assumed equal
    // (coinduction). The answer must be equal, and the call must return.
    #[test]
    fn structurally_equal_terminates_on_identical_recursive_types() {
        let mut arena = TypeArena::new();
        let null = arena.null();
        let a = recursive_node(&mut arena, null);
        let b = recursive_node(&mut arena, null);

        assert_ne!(a, b);
        assert!(arena.structurally_equal(a, b));
    }

    // Same, but the shapes really differ (null vs undefined tail). Must
    // terminate AND say not equal: assuming "equal" for a re-entered pair must
    // not hide a difference elsewhere in the type.
    #[test]
    fn structurally_equal_terminates_on_different_recursive_types() {
        let mut arena = TypeArena::new();
        let (null, undefined) = (arena.null(), arena.undefined());
        let a = recursive_node(&mut arena, null);
        let c = recursive_node(&mut arena, undefined);

        assert!(!arena.structurally_equal(a, c));
    }

    // The realistic way to reach it: `type Either = A | B` goes through
    // alloc_union, whose dedup calls structurally_equal on the members.
    #[test]
    fn alloc_union_of_identical_recursive_types_terminates() {
        let mut arena = TypeArena::new();
        let null = arena.null();
        let a = recursive_node(&mut arena, null);
        let b = recursive_node(&mut arena, null);

        let union = arena.alloc_union(vec![a, b]);

        assert!(matches!(arena.get(union), Type::Object(_) | Type::Union(_)));
    }

    // A method and a function-valued property must stay distinct types. The intern
    // key separates them (PropertyEntry hashes is_method) and subtyping treats them
    // differently (method parameters are bivariant), so structurally_equal has to
    // compare is_method too: otherwise alloc_union would merge the two into one
    // union member, while tsc keeps both.
    #[test]
    fn a_method_and_a_function_valued_property_are_not_the_same_shape() {
        let mut arena = TypeArena::new();
        let void = arena.void();
        let function = arena.alloc(Type::Function(FunctionType {
            params: vec![],
            return_type: void,
            is_untyped: false,
        }));
        let entry = |is_method: bool| PropertyEntry {
            name: "f".into(),
            type_id: function,
            optional: false,
            is_method,
        };

        // The intern key agrees they differ (always held) ...
        let interned_method = arena.alloc(Type::Object(ObjectType::new(vec![entry(true)])));
        let interned_field = arena.alloc(Type::Object(ObjectType::new(vec![entry(false)])));
        assert_ne!(interned_method, interned_field);

        // ... and structural equality must agree with it: is_method is part of the shape.
        let method = object(&mut arena, vec![entry(true)]);
        let field = object(&mut arena, vec![entry(false)]);
        assert!(!arena.structurally_equal(method, field));
    }

    // make_unique leaves the ten fixed slots alone (they are not internable), and
    // namespace::resolve names whatever it gets back, so without a guard
    // `type Age = number` would name slot 0 and print every `number` as "Age".
    // set_display_name ignores the fixed slots.
    #[test]
    fn a_fixed_slot_never_takes_a_display_name() {
        let mut arena = TypeArena::new();
        let fixed = [
            arena.number(),
            arena.string(),
            arena.boolean(),
            arena.null(),
            arena.undefined(),
            arena.any(),
            arena.unknown(),
            arena.error(),
            arena.never(),
            arena.void(),
        ];

        for id in fixed {
            let unique = arena.make_unique(id);
            arena.set_display_name(unique, "Renamed");
            assert_eq!(
                arena.display_name(id),
                None,
                "fixed slot {id:?} was renamed"
            );
        }
    }
}
