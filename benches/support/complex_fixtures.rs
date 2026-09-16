// Shared source generators for Criterion benchmarks, IAI benchmarks,
// and heap profiling.
//
// These fixtures intentionally use the TypeScript constructs currently
// exercised by the checker:
//
// - interfaces and structural object types
// - discriminated unions
// - object-property access
// - control-flow narrowing
// - generic inference and substitution
// - arrays
// - object destructuring
// - classes and inheritance
// - method calls
// - cross-function references
//
// The connected_application_source generator combines those features into
// one interdependent application-like workload instead of measuring each
// feature in isolation.

pub fn wide_discriminated_union_source(variant_count: usize) -> String {
    let variant_count = variant_count.max(1);
    let mut src = String::with_capacity(variant_count * 180);

    for i in 0..variant_count {
        src.push_str(&format!(
            "interface Variant{i} {{\n\
             \tkind: \"variant{i}\";\n\
             \tpayload{i}: number;\n\
             }}\n\n"
        ));
    }

    src.push_str("type Wide = ");
    for i in 0..variant_count {
        if i > 0 {
            src.push_str(" | ");
        }

        src.push_str(&format!("Variant{i}"));
    }
    src.push_str(";\n\n");

    src.push_str(
        "function handle(value: Wide): number {\n\
         \tif (value.kind === \"variant0\") {\n\
         \t\treturn value.payload0;\n\
         }\n",
    );

    for i in 1..variant_count {
        src.push_str(&format!(
            "\telse if (value.kind === \"variant{i}\") {{\n\
             \t\treturn value.payload{i};\n\
             }}\n"
        ));
    }

    src.push_str("\n\treturn 0;\n}\n");

    src
}

pub fn nested_object_source(depth: usize) -> String {
    let depth = depth.max(1);
    let mut src = String::with_capacity(depth * 100);

    src.push_str(
        "interface Level0 {\n\
         \tvalue: number;\n\
         }\n\n",
    );

    for i in 1..depth {
        let previous = i - 1;

        src.push_str(&format!(
            "interface Level{i} {{\n\
             \tinner: Level{previous};\n\
             \ttag{i}: number;\n\
             }}\n\n"
        ));
    }

    let last = depth - 1;

    src.push_str(&format!(
        "function unwrap(value: Level{last}): number {{\n\
         \treturn value.tag{last};\n\
         }}\n"
    ));

    src
}

pub fn class_hierarchy_source(depth: usize) -> String {
    let depth = depth.max(1);
    let mut src = String::with_capacity(depth * 150);

    src.push_str(
        "class Base0 {\n\
         \tfield0: number = 0;\n\n\
         \tdescribe0(): number {\n\
         \t\treturn this.field0;\n\
         \t}\n\
         }\n\n",
    );

    for i in 1..depth {
        let previous = i - 1;

        src.push_str(&format!(
            "class Base{i} extends Base{previous} {{\n\
             \tfield{i}: number = {i};\n\n\
             \tdescribe{i}(): number {{\n\
             \t\treturn this.field{i} + this.describe{previous}();\n\
             \t}}\n\
             }}\n\n"
        ));
    }

    let last = depth - 1;

    src.push_str(&format!(
        "function total(instance: Base{last}): number {{\n\
         \treturn instance.describe{last}();\n\
         }}\n"
    ));

    src
}

pub fn generic_heavy_source(call_count: usize) -> String {
    let call_count = call_count.max(1);

    let mut src = String::from(
        "function identity<T>(value: T): T {\n\
         \treturn value;\n\
         }\n\n\
         function firstOf<T>(items: T[]): T {\n\
         \treturn items[0];\n\
         }\n\n\
         function choose<T>(condition: boolean, left: T, right: T): T {\n\
         \tif (condition) {\n\
         \t\treturn left;\n\
         \t}\n\n\
         \treturn right;\n\
         }\n\n",
    );

    for i in 0..call_count {
        match i % 3 {
            0 => {
                src.push_str(&format!("const genericValue{i}: number = identity({i});\n"));
            }
            1 => {
                src.push_str(&format!(
                    "const genericValue{i}: number = firstOf([{i}, {}, {}]);\n",
                    i + 1,
                    i + 2
                ));
            }
            _ => {
                src.push_str(&format!(
                    "const genericValue{i}: number = choose(true, {i}, {});\n",
                    i + 1
                ));
            }
        }
    }

    src
}

pub fn destructuring_heavy_source(binding_count: usize) -> String {
    let binding_count = binding_count.max(1);

    let mut src = String::from(
        "interface Record {\n\
         \ta: number;\n\
         \tb: string;\n\
         \tc: boolean;\n\
         \td: number;\n\
         }\n\n\
         function source(): Record {\n\
         \treturn { a: 1, b: \"x\", c: true, d: 2 };\n\
         }\n\n",
    );

    for i in 0..binding_count {
        src.push_str(&format!(
            "const {{ a: a{i}, b: b{i}, c: c{i}, d: d{i} }} = source();\n"
        ));
    }

    src
}

pub fn complex_source(scale: usize) -> String {
    let scale = scale.max(1);
    let mut src = String::new();

    src.push_str(&wide_discriminated_union_source(scale));
    src.push_str("\n\n");

    src.push_str(&nested_object_source(scale));
    src.push_str("\n\n");

    src.push_str(&class_hierarchy_source(scale));
    src.push_str("\n\n");

    src.push_str(&generic_heavy_source(scale * 4));
    src.push_str("\n\n");

    src.push_str(&destructuring_heavy_source(scale * 2));

    src
}

// A connected, application-like workload.
//
// The generated declarations are not independent. Values flow through:
//
// interface composition
// -> union parameters
// -> generic inference
// -> generic substitution
// -> object property access
// -> control-flow narrowing
// -> destructuring
// -> class methods
// -> cross-function calls
//
// Only constructs already represented in the existing successful benchmark
// fixtures are used here. More advanced unsupported TypeScript features should
// be tested separately rather than making this benchmark fail for unrelated
// parser or checker limitations.
pub fn connected_application_source(scale: usize) -> String {
    let scale = scale.max(1);
    let mut src = String::with_capacity(scale * 3_000);

    src.push_str(
        r#"
interface User {
    id: number;
    name: string;
    active: boolean;
}

interface Admin {
    id: number;
    name: string;
    active: boolean;
    permissions: number;
}

interface Guest {
    id: number;
    name: string;
    active: boolean;
    expiresAt: number;
}

type Account = User | Admin | Guest;

interface UserResponse {
    ok: boolean;
    account: User;
}

interface AdminResponse {
    ok: boolean;
    account: Admin;
}

interface GuestResponse {
    ok: boolean;
    account: Guest;
}

type Response = UserResponse | AdminResponse | GuestResponse;

function identity<T>(value: T): T {
    return value;
}

function first<T>(items: T[]): T {
    return items[0];
}

function choose<T>(condition: boolean, left: T, right: T): T {
    if (condition) {
        return left;
    }

    return right;
}

function accountId(account: Account): number {
    return account.id;
}

function accountScore(account: Account): number {
    if (account.active) {
        if (account.name === "admin") {
            return account.id + 100;
        }

        return account.id + 10;
    }

    return 0;
}

function responseScore(response: Response): number {
    if (response.ok) {
        return accountScore(response.account);
    }

    return 0;
}

class AccountStore {
    primary: Account;
    backup: Account;

    constructor(primary: Account, backup: Account) {
        this.primary = primary;
        this.backup = backup;
    }

    getPrimary(): Account {
        return this.primary;
    }

    getBackup(): Account {
        return this.backup;
    }

    scoreBoth(): number {
        return accountScore(this.primary) + accountScore(this.backup);
    }
}

function inspectAccount(account: Account): number {
    const normalized = identity(account);
    const selected = choose(true, normalized, account);
    const copied = identity(selected);

    return accountId(copied) + accountScore(copied);
}
"#,
    );

    for i in 0..scale {
        let admin_id = i + 1;
        let guest_id = i + 100;
        let permissions = i + 10;
        let expires_at = i + 1_000;

        src.push_str(&format!(
            r#"
function makeUser{i}(): User {{
    return {{
        id: {i},
        name: "user{i}",
        active: true
    }};
}}

function makeAdmin{i}(): Admin {{
    return {{
        id: {admin_id},
        name: "admin",
        active: true,
        permissions: {permissions}
    }};
}}

function makeGuest{i}(): Guest {{
    return {{
        id: {guest_id},
        name: "guest{i}",
        active: false,
        expiresAt: {expires_at}
    }};
}}

function processAccount{i}(input: Account): number {{
    const normalized = identity(input);
    const selected = choose(true, normalized, input);
    const result = inspectAccount(selected);

    if (selected.active) {{
        if (selected.name === "admin") {{
            return result + selected.permissions;
        }}

        return result + selected.id;
    }}

    return result;
}}

function processResponse{i}(input: Response): number {{
    const response = identity(input);

    if (response.ok) {{
        const account = response.account;
        const copied = identity(account);

        if (copied.active) {{
            return copied.id + accountScore(copied);
        }}

        return copied.id;
    }}

    return 0;
}}

function runBatch{i}(): number {{
    const user = makeUser{i}();
    const admin = makeAdmin{i}();
    const guest = makeGuest{i}();

    const accounts: Account[] = [user, admin, guest];

    const firstAccount = first(accounts);
    const selectedAccount = choose(true, firstAccount, admin);
    const normalizedAccount = identity(selectedAccount);

    const {{ id: userId, active: userActive }} = user;
    const {{ id: adminId, permissions }} = admin;
    const {{ id: guestId, expiresAt }} = guest;

    const store = new AccountStore(normalizedAccount, guest);

    return (
        processAccount{i}(user)
        + processAccount{i}(admin)
        + processAccount{i}(guest)
        + inspectAccount(firstAccount)
        + inspectAccount(selectedAccount)
        + inspectAccount(normalizedAccount)
        + processResponse{i}({{
            ok: true,
            account: user
        }})
        + processResponse{i}({{
            ok: true,
            account: admin
        }})
        + processResponse{i}({{
            ok: true,
            account: guest
        }})
        + responseScore({{
            ok: true,
            account: user
        }})
        + store.scoreBoth()
        + userId
        + adminId
        + guestId
        + permissions
        + expiresAt
        + (userActive ? 1 : 0)
    );
}}

const result{i}: number = runBatch{i}();
"#,
        ));
    }

    src
}
