# 22 — Expose template scanning and single-variable lookup from core

**Repo:** .
**Depends on:** none
**Owns:**
- crates/core/src/template.rs (new)
- crates/core/src/lib.rs (insert `pub mod template;` and re-exports only)
- crates/core/src/resolve/** (refactor to share the scanner; add `lookup_var`)

## Goal
The UI must highlight `{{var}}` and `{name}` in the URL bar, show the resolved value on hover, and keep `[path]` in sync with the URL. Those positions and values must agree exactly with what `resolve()` does, so the scanner moves into a public `template` module that `resolve` itself uses, and `resolve` gains a lookup of one variable through the same precedence and expansion bounds.

## Context
- crates/core/src/resolve/engine.rs:42-56 — `lookup`, precedence
- crates/core/src/resolve/engine.rs:96-195 — `subst`/`expand` (bounded expansion: depth 8, 64 KiB, 10,000 budget)
- crates/core/src/resolve/engine.rs:220-370 — path portion detection and path-param parsing (`{id}`, `{id:\d+}`, `{*path}`, several per segment)
- crates/core/src/resolve/mod.rs:1-62 — public resolve types
- crates/core/src/resolve/tests.rs — existing behaviour that must not change
- Build with `CARGO_TARGET_DIR=/Users/Andrew/HttpClient/target/wt-22`.

## Acceptance
- [ ] `wisp_core::template` exposes, with byte ranges on char boundaries into the input:
  `pub struct VarRef { pub range: Range<usize>, pub name: String, pub terminated: bool }` (range covers the braces), `pub fn var_refs(s: &str) -> Vec<VarRef>`;
  `pub struct PathParamRef { pub range: Range<usize>, pub name: String, pub pattern: Option<String>, pub catch_all: bool }`, `pub fn path_params(url: &str) -> Vec<PathParamRef>`;
  `pub fn path_range(url: &str) -> Range<usize>` and `pub fn query_start(url: &str) -> Option<usize>` (first `?` that is not inside `{{...}}`).
- [ ] `resolve`'s variable and path-param handling call these functions (or one private scanner they share); `rg -n 'fn ' crates/core/src/resolve/engine.rs` shows no second `{{` scanner.
- [ ] `wisp_core::resolve` exposes `pub fn lookup_var(request: &Request, scope: &Scope, name: &str) -> VarLookup` with
  `pub enum VarSource { Request, Folder(usize), Environment, Secret, Collection }` (`Folder` indexes `scope.folders`, outermost = 0) and
  `pub enum VarLookup { Found { value: String, source: VarSource, secret: bool }, Dynamic, Unresolved, Error(Problem) }`; `value` is fully expanded with the same bounds as `resolve`; `secret` is true when the variable or any variable its expansion used came from `Environment::secrets`.
- [ ] `VarSource`, `VarRef` and `PathParamRef` derive `Debug, Clone, PartialEq, Eq`; `VarLookup` derives `Clone, PartialEq, Eq` and has a manual `Debug` that prints `value` as `"<redacted>"` when `secret` is true; a unit test asserts the secret value does not appear in `format!("{:?}", lookup)`.
- [ ] Unit tests cover: `{{a}}{{b}}` adjacent, unterminated `{{a`, `{{ a }}` name trimming identical to `resolve`, `$uuid`/`$timestamp` reported as `Dynamic`, a `?` inside `{{...}}` not counted by `query_start`, `{a}-{b}` and `{name}.{ext}` in one segment, `{id:\d+}` pattern capture, `{*path}` catch-all, `{x}` in the query not reported by `path_params`, a cycle reported as `Error(Problem::VarCycle{..})`, and precedence request > folder (nearest) > secret > environment > collection.
- [ ] Every test in `crates/core` that passed at `49da271` still passes unchanged (no edits to existing assertions in `resolve/tests.rs`).
- [ ] `cargo fmt --check` and `cargo clippy --workspace --all-targets -- -D warnings` are clean.

## Out of scope
- The resolve nits carried forward in PLAN (verbose-mode regex, Debug-string dedupe, leftover `eprintln`, scheme-less `://`) unless the refactor removes one as a side effect.
- Any change to `PreparedRequest`, `Event` or `Command`.
- New dependencies.
- Anything in `crates/app` or `crates/storage`.
