# Security

SketchCraft opens files people send each other (SketchUp, FBX, DXF, DWG, OBJ)
and can be driven by tools (MCP, the dev bridge). This page says what's
exposed, what has been tested, and how to test it again.

## What's exposed

| Surface | Who can reach it | Guard |
| --- | --- | --- |
| Desktop app (Tauri) | the app's own window | three IPC calls only (`exec`, `scene`, `sync`); `core:default` + `dialog:default` permissions; strict Content-Security-Policy (own files only, no remote scripts) |
| Dev bridge (`sketchcraft bridge`, 127.0.0.1:5175; development only, the app doesn't start it) | programs on this Mac | every request (GET too) needs a random per-launch token from a file only you can read; refuses any browser `Origin` but the dev page; refuses any `Host` but 127.0.0.1/localhost (no DNS rebinding); 64 MB request cap |
| MCP server | the agent you connect | stdio only; no network listener; modeling plus file import/export **inside one workspace folder** (the model's folder, `--workspace DIR`, or `--any-path`), never plugin management, raw file writes, or starting programs (`animate`, `import_ifc`, `import_dwg` and `export_dwg` are the person's alone); `--any-path` lifts only the folder limit, not the rules about hidden files, Backups and Plugins; names read from a model, in every reply, are cut and marked as data |
| Plugins (JavaScript and SketchUp Ruby) | whoever wrote the plugin you installed | a worker with no network, no page, no way to start another worker; every command it sends runs under an **allowlist in the engine** (`core/src/policy.rs`): modeling and inspection only. Installing shows what it is first; replacing one needs a yes |
| File importers | anyone who sends you a file | fuzzed (below); limits on sizes, depth and parse time |
| External programs (ffmpeg, Python for IFC, DWG converters, curl, `open`) | paths you choose | only you can start them, never a plugin or an agent; arguments, never a shell; paths can't be read as flags or be URLs, pipes or devices; each has a hard time limit and is killed, with everything it started, when it runs over or when SketchCraft or the MCP server exits; Reveal shows files, never launches them (on Windows too) |
| The model file | every program that edits it | written whole-or-not-at-all, never with non-finite numbers, with rolling backups in `Backups/`; a file that can't be read is copied aside and reported |
| Updates and releases | GitHub, the maintainer's signing key | signed with a pinned key; the app only accepts a newer version whose downloads come from that version's own release; release builds run from a `v*` tag on `main` with pinned actions, a pinned Rust version, least-privilege tokens, SHA-256 checksums and build-provenance attestations (`gh attestation verify`, docs/SIGNING.md); the signing step takes each platform's files from its own download and refuses unexpected names or file types |

## Review, 2026-11-13

| # | Finding | Severity | Fix |
| --- | --- | --- | --- |
| 1 | Bridge ran commands from any website the user visited (a plain POST needs no CORS preflight), and was open to DNS rebinding: arbitrary file writes | Critical | Origin + Host checks, body cap (`core/src/http.rs`) |
| 2 | Desktop app had no Content-Security-Policy | High | strict CSP (`app/src-tauri/tauri.conf.json`) |
| 3 | A 1 KB FBX with a node length pointing backwards looped until the app ran out of memory | High | node ends must follow their start (`core/src/fbx.rs`); regression test |
| 4 | FBX arrays reserved their declared size up front and inflated zlib data without a ceiling | Medium | reserve as data arrives, stop at the declared size, reject mismatches |
| 5 | A damaged SketchUp file kept the importer searching for 29 s | Medium | a whole-file deadline (4 s + 4 s/MB; real files take < 0.4 s) |
| 6 | Paths starting with `-` became options to ffmpeg / `open` / converters; Reveal could launch an `.app` | Medium | `safety::path_arg`; Reveal uses `open -R` for bundles |
| 7 | Video frames were unbounded | Low | frame index and size limits |
| 8 | A panic in one command would poison the engine lock and break the app | Low | IPC calls and the bridge catch panics and answer with an error |
| 9 | `zip` 2.6 (RUSTSEC-2026-0367/0368/0369/0429; we never called `extract`), unsound `i_tree` via `geo` | Medium (deps) | zip 9, geo 0.33 |

Not fixed, and why: `glib` (unsound) and `proc-macro-error` (unmaintained)
come from Tauri's Linux GTK layer, aren't in the Mac build, and need a Tauri
release. TypeScript 7 (a new compiler) is left for a deliberate upgrade.

## Review, 2026-11-16 (red team)

Four assessments (pentest, APT, financial, chaos) found 53 findings, 34 once
the overlaps are merged. All are fixed, mitigated or accepted with a reason,
except two that need the repository owner (code signing and the update key's
protection, see [Signing](SIGNING.md)). In short:

- One command policy in the engine (`core/src/policy.rs`) for plugins, SketchUp
  Ruby and agents; allowlists, not denylists. Numbers, strings and paths in every
  command are checked; a result with a non-finite or huge number is undone.
- The model can't be lost to one bad command: panics restore the model, saves
  are atomic, backups roll, damaged files are kept.
- Bridge token, MCP workspace, plugin sandbox lockdown, install preview, error
  boundary, skin value grammar, hashed Ruby cache, resource budgets (render,
  photoreal, DXF blocks, undo memory), updater and release-workflow hardening.

A second round of review (Black Team round 2) found more, all fixed or recorded for the
owner: reloads and Undo no longer lose another client's work, backups can't be
flushed, a plugin run is one Undo step, a DXF that multiplies a block is refused
quickly, agents write only the right kind of file under new names (never over
yours) and can't be steered by an OBJ's material line, the dev server's token
endpoint is limited to its own page and user, and the release workflow keeps the
update key away from compilation. See the red-team notes in the repository history.

A fourth round: backups have a quota per origin (you, an agent, a plugin), are paced and
capped by bytes, and a plugin run that wipes the model is backed up first; reserved folders
(`Backups`, `Plugins`) are compared folded and by identity; the "made by this agent" record is
kept only for files really written and checked by file identity; a model holds at most 2 million
entities (a bigger one opens for viewing and cutting down); DXF wall layers and hatches and
`text_3d` stop at their deadline; Undo is per client (see `core/src/history.rs`); the release
workflow's compiling job no longer names an Apple secret.

Checks for these live in `core/tests/hardening.rs`, `core/tests/undo.rs`, `app/checks/run.mjs`
(`npm run check`) and `tools/security-tests/plugin-sandbox.mjs`.

Known limits: plugins are trusted with your model (not your files): install
only ones you trust; the dev bridge is for development, and a plugin run in the
plain browser dev page has the same limits but the page can't tell it apart from
the app's own calls if it escapes the worker; macOS builds aren't notarized and
Windows builds aren't code-signed yet (see [Signing](SIGNING.md)).

## Testing it again

```sh
cargo audit                                   # Rust advisories (0 vulnerabilities on 2026-11-13)
(cd app && npm audit)                         # npm advisories (0)
cargo test --release -p sketchcraft-core --test hardening      # integrity, policy, limits
(cd app && npm run check)                                      # skin values, plugin lockdown, plugin data
SKETCHCRAFT_DOC=/some/scratch/live.sketchcraft node tools/security-tests/plugin-sandbox.mjs   # a hostile plugin in a real browser (see dev/building.md)
cargo test --release --test fuzz -- --ignored --nocapture       # DXF, OBJ (20,000 cases each), SketchUp samples
cargo test --release --test fbx -- --ignored --nocapture        # FBX (20,000 cases)
FUZZ_TRACE=1 FUZZ_SKP_ITERS=150 cargo test --release --test fuzz fuzz_skp -- --ignored --nocapture
```

The fuzzers save any input that panics or takes over 2 s to `out/fuzz/`;
`FUZZ_TRACE=1` also writes each input before running it, so a crash that
can't be caught (an abort, running out of memory) leaves its input behind.
SketchUp fuzzing uses your own sample files and never commits them.

Report a problem privately: open a GitHub security advisory on the repo.
