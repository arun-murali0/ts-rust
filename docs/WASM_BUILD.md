# Building & publishing `ts-rust` for npm

`ts-rust` is a native Rust crate by default. The `wasm` feature adds a
`wasm-bindgen` boundary (`src/wasm.rs`) so it can also ship as an npm
package via [`wasm-pack`](https://rustwasm.github.io/wasm-pack/).

## Native (library / tests / CI)

```sh
cargo test
cargo build --release
```

No wasm dependencies are pulled in for these. `wasm-bindgen`, `serde`,
etc. are all behind `--features wasm` and off by default.

## WASM / npm build

Requires `wasm-pack` (`cargo install wasm-pack --locked`).

```sh
wasm-pack build --target web --features wasm
```

Don't pass `--out-dir pkg` explicitly. It's already the default, and on
some `wasm-pack`/cargo toolchain combinations an explicit `--out-dir`
triggers a code path that proxies the value straight through to
`cargo build --out-dir`, an unstable cargo flag gated behind
`-Z unstable-options` on nightly. On a normal stable toolchain that fails
with `error: unexpected argument '--out-dir' found`. Omitting the flag
avoids the code path entirely; the default output directory is
unaffected.

- `--target web` produces an ES module suitable for bundlers and modern
  browsers. Use `--target nodejs` instead for a CommonJS build if the
  primary consumers are Node scripts rather than bundled web apps. We may
  eventually publish both under different `exports` conditions.
- Output lands in `pkg/`: `ts_rust_bg.wasm`, `ts_rust.js`, `ts_rust.d.ts`
  (underscored, matches the `[lib] name = "ts_rust"` in Cargo.toml; the
  npm package name itself stays `ts-rust`),
  and a generated `package.json`. `pkg-template/package.json` in this repo
  is a reference for fields wasm-pack won't infer (description, license,
  repository). Merge relevant fields in, or pass `--out-name`, or
  edit `Cargo.toml`'s `[package.metadata.wasm-pack]` to control this instead
  of hand-editing generated output each release.

## Publish

```sh
cd pkg
npm publish --access public
```

## Usage from JS (once published)

```js
import init, { TsRustChecker } from "ts-rust";

await init(); // loads the wasm binary
const checker = new TsRustChecker();
const diagnostics = checker.checkSource("const x: number = 'oops';", "input.ts");
console.log(diagnostics);
```

## CI note

Two separate jobs in `.github/workflows/ci.yml`:
1. `cargo test --all-targets` (native, default features): fast, runs on
   every push.
2. `cargo install wasm-pack --locked` then
   `wasm-pack build --target web --features wasm`, runs the wasm boundary
   specifically, since bugs there (serde-wasm-bindgen shape mismatches,
   JsValue conversions) won't show up in native tests. `wasm-pack` is
   installed fresh from crates.io on every run rather than pinned via a
   third-party action, so it always matches whatever toolchain the runner
   has. A stale cached binary was the root cause of the `--out-dir`
   failure described above.
