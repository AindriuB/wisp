# 77 — Build the Windows release as a GUI-subsystem exe with a static CRT

**Repo:** `.`
**Depends on:** none
**Base:** `main` at `92c6047` (branch `task/77-windows-gui-crt`).
**Build dir:** `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-77`
**Owns:**
- crates/app/src/main.rs (one new crate-level attribute line under the `//!` doc
  comment; nothing else in the file)
- .cargo/config.toml (a new `[target.x86_64-pc-windows-msvc]` section only; the
  existing Linux section stays byte-for-byte unchanged)
- .github/workflows/inspect-windows-77.yml (optional, throwaway; must be deleted
  on the branch before the task is handed to `/record`)
- docs/plan/tasks/77-windows-gui-crt.md (the `## Notes` section only)

## Goal
The README review found two Windows release bugs. First, `wisp.exe` is a console-subsystem
binary, so launching it from Explorer also opens a console window. Second, it links the
MSVC CRT dynamically, so it needs `vcruntime140.dll` (the VC++ redistributable) and may
not start on a clean Windows install. Release builds should use the GUI subsystem and a
static CRT. `measure.ps1` must keep reading `WISP_EVENT` lines and the size budget must
still hold.

## Context
- crates/app/src/main.rs:1 — the crate doc comment; the attribute goes directly below it.
  Suggested form: `#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]`
  (debug builds keep a console for `cargo run` diagnostics). A different condition is fine
  if Notes says why.
- .cargo/config.toml:1-3 — holds only task 64's `[target.x86_64-unknown-linux-gnu]` link
  flags. Add `[target.x86_64-pc-windows-msvc]` with
  `rustflags = ["-C", "target-feature=+crt-static"]`. A `RUSTFLAGS` env var would override
  config rustflags completely, and `rg -n RUSTFLAGS .github/` currently finds nothing. Keep it that way.
- crates/app/src/workspace.rs:135,571-594 — `WISP_EVENT` lines go to **stdout** (`println!`);
  workspace.rs:124 and the `WISP_TRACE` diagnostics go to stderr.
- scripts/measure/measure.ps1:148,161,171-178 — launches the app with `Start-Process
  -RedirectStandardOutput/-RedirectStandardError` and polls the stdout log for
  `^WISP_EVENT completed`. A GUI-subsystem process has no console, but handles passed in
  through STARTUPINFO by `Start-Process` redirection are inherited, and Rust's
  `GetStdHandle`-based stdout should still write to them. That is the claim to prove. If it
  fails, fix it in the app (for example, attach to the inherited handles or the parent
  console when they are present) in main.rs. Do not drop the attribute, and amend Owns
  first if the fix needs another file.
- .github/workflows/measure.yml:26-35 — the Windows job builds `--release --target
  x86_64-pc-windows-msvc`, so it gets both changes. ci.yml runs on ubuntu-24.04 and
  macos-latest only, so it does not build Windows.
- .github/workflows/release.yml:6-8,29-32,120-121 — `workflow_dispatch` with the
  `draft_release` input; the Windows matrix row; the dispatch draft-release step.
- docs/architecture.md, the "After task 64" table — Windows `x86_64-pc-windows-msvc`
  13,450,240 B, the baseline for the size comparison. The budget is 16 MiB.
- Task 76 (open, on its own branch) adds `crates/app/build.rs` and a `winresource`
  build-dependency for the icon. This task must not touch either.

## Acceptance
- [ ] `git diff main -- crates/app/src/main.rs` adds exactly one line, a
      `windows_subsystem = "windows"` attribute gated so that it applies to Windows release
      builds. If a stdout/stderr fix turned out to be needed, the diff also contains that
      fix and Notes explains it.
- [ ] `git diff main -- .cargo/config.toml` adds only a `[target.x86_64-pc-windows-msvc]`
      section with `rustflags = ["-C", "target-feature=+crt-static"]`. `rg -n RUSTFLAGS
      .github/` prints nothing.
- [ ] `git diff main --stat` touches no `build.rs`, no `Cargo.toml` and no `Cargo.lock`,
      and no file under `.github/` (the throwaway workflow is gone).
- [ ] A `measure.yml` run on this branch (`gh workflow run measure.yml --ref
      task/77-windows-gui-crt`) has all three jobs `measured` and within budget. The Windows
      job saw `WISP_EVENT completed`. Notes records the run id and the Windows binary size
      before (13,450,240 B, or a fresh main run if one is newer) and after, in bytes and as
      a percentage change, and the after size is ≤ 16,777,216 B.
- [ ] A `release.yml` dispatch on this branch (`gh workflow run release.yml --ref
      task/77-windows-gui-crt -f draft_release=true`) succeeds. Its Windows `wisp.exe`,
      checked with `dumpbin /headers`, shows subsystem `2 subsystem (Windows GUI)`, and
      `dumpbin /dependents` lists no `VCRUNTIME140*.dll`, `MSVCP140*.dll` or
      `api-ms-win-crt-*.dll`. Run dumpbin on a windows runner (throwaway workflow above,
      run on the branch, which downloads the release artefact or rebuilds the same way),
      or on the Mac against the downloaded `pkg-windows-x64` artefact with
      `/usr/bin/objdump -p` (Apple LLVM objdump: `Subsystem` field and the import table).
      Notes records which tool was used, the run ids, and the relevant output lines. The
      draft release that the dispatch created is deleted afterwards (`gh release delete
      <tag> --yes`, and its tag if one was created). No tag is pushed and nothing is
      published.
- [ ] `ci.yml` is green on the branch head.
- [ ] No PR is opened by the implementer.

## Out of scope
- The Windows icon and version resource, `crates/app/build.rs` and `winresource` (task 76).
- Code signing and SmartScreen.
- Any change to `measure.yml`, `release.yml`, `ci.yml`, `measure.ps1` or `package.sh`.
- Adding a Windows job to `ci.yml`.
- Updating the budget tables in `docs/architecture.md` or the README. The scribe does
  that at `/record` using the figures in Notes.
- The Linux section of `.cargo/config.toml` and any further size work.

## Notes

- Attribute: `#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]`
  as suggested; debug builds keep the console. No stdout/stderr fix was needed.
- measure.yml run 41704961344 (commit e71314a): all three jobs `measured`, within budget.
  The Windows job log `measure-x86_64-pc-windows-msvc.req.out.log` contains
  `WISP_EVENT completed status=200 size=65536`, so redirected stdout reaches the log from the
  GUI-subsystem process.
- Windows binary size: before 13,450,240 B (docs/architecture.md, after task 64), after
  13,630,464 B, +180,224 B (+1.34%). Budget 16,777,216 B. Idle private 35,924 KiB, after
  request 41,488 KiB.
- release.yml dispatch run 41704966546 succeeded. Tool: Apple `/usr/bin/objdump -p` on the
  downloaded `pkg-windows-x64` wisp.exe (13,630,464 B). Output: `Subsystem 00000002 (Windows GUI)`.
  Import table lists only system DLLs (kernel32, advapi32, ntdll, ws2_32, user32, gdi32,
  d3d11, dxgi, dwrite, ...); no VCRUNTIME140*, MSVCP140* or api-ms-win-crt-* entries
  (`api-ms-win-core-*`/`shcore`/`winrt` are OS API sets, not the CRT). No inspect workflow
  was needed.
- Draft release `dryrun-41704966546` deleted; no tag was created; `gh release list` is empty.
- ci.yml run on branch head e71314a: success.
