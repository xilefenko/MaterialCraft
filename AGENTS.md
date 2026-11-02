# AGENTS.md: guide for AI agents and contributors

**MaterialCraft** is a procedural, node-based material editor written in Rust, with a Paint mode for layered texture
painting on meshes that can import Substance 3D Painter projects. It is a member of the Craft app suite (PhotoCraft
and siblings) and follows the suite's conventions. Write the product name as **MaterialCraft** in user-facing text;
machine names stay lowercase (`materialcraft`, `materialcraft-substance`, `mcsub`).

Internal research, design docs, decisions and the roadmap live **outside this repository** (in `MaterialCraft-docs`
next to it). Never copy them into the repo; refer to them by path in comments when needed.

## Workspace map

```text
src/                      materialcraft: std-only core library + the `materialcraft` CLI (src/main.rs)
  graph nodes2 ops ...    node graph and procedural nodes
  paint/                  Paint mode document model: tiled images, channels, layers, compositing, .mcpaint format
  mesh/ bake render3d     meshes (OBJ/glTF), baking, CPU 3D preview
  gpu3d/                  OpenGL viewport through glow (feature "gui" only; used by the editor and examples)
  gpu.rs                  GPU compute for node evaluation through wgpu (feature "gpu"; CPU code stays the reference)
crates/editor             materialcraft-editor: the editor binary `materialcraft-gui`
  src/main.rs             node editor; include!s gui_params/gui_ui/gui_home.rs
  src/gui_paint/          Paint mode screen
  src/gui_theme.rs        the editor theme
crates/spp-reader         spp-reader: reads .spp projects (container, records, Painter's layer model, blocker keys); no app dependency
crates/substance          materialcraft-substance: imports .spp into paint documents (translate, catalog); `mcsub`
crates/parity             materialcraft-parity: `mcparity`, measures our results against Painter's on local projects
crates/starter            materialcraft-starter: `mcstarter`, builds the starter asset library (content/materials/*.mcg)
```

## Build and test

```sh
cargo build --workspace --all-features
cargo test --workspace
cargo clippy --workspace --all-targets --all-features
cargo fmt --all
cargo run --release -p materialcraft-editor
cargo run --release -p materialcraft-substance --bin mcsub -- convert <in.spp> <out.mcpaint>
cargo run --release -p materialcraft-starter -- <folder>   # the starter library; the editor also builds it on first run
```

Toolchain: Rust 1.95 or newer, edition 2024. Formatting follows `rustfmt.toml` (wide lines, as in the other Craft apps).

## Rules

1. **Clean room.** Nothing proprietary goes into the repository: no Adobe/Substance code, data, shaders, presets,
   resources or documentation text. Everything we need is written from scratch from observed behaviour and open
   standards. Substance files are only *read* by the importer (`crates/substance`) at run time; nothing read from a
   user's project (images, numbers, corpus files) is ever committed.
2. **Module boundaries.**
   - The paint model (`src/paint`) knows no concrete procedural; procedurals plug in behind its interfaces.
   - Mapping tables from Painter's concepts to ours live only in the importer's translate stage
     (`crates/substance/src/translate`); the reader below it (`crates/spp-reader`) knows nothing about
     MaterialCraft and must not depend on any MaterialCraft crate.
   - The editor is thin: it shows state and turns input into calls on the model; logic lives in the libraries.
3. **Object-oriented and decoupled.** Small types with one job each, wired together explicitly. No god objects or god
   files: split a file before it grows into one.
4. **No drive-by refactors.** A change does what it says. Refactors, renames and formatting go in their own commits.
5. **Never crash (new code).** Non-test code in new crates doesn't panic on any input: no `unwrap()`, `expect()`,
   `panic!`, `todo!`, `unimplemented!`, `unreachable!` or unchecked indexing. Return an error and propagate it with
   `?`. New crates start their root with
   `#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::unimplemented, clippy::todo, clippy::unreachable)]`
   (`clippy.toml` allows them in tests). Existing code isn't converted wholesale; tighten it when you touch it, in
   its own commit.
6. **No `unsafe`.** The workspace sets `unsafe_code = "forbid"`. The root package uses `deny` instead because of two
   documented exceptions with a module-level `allow`:
   - `src/gpu3d`: glow exposes every OpenGL call as `unsafe fn`; the module's doc states the invariant (calls only
     inside egui_glow paint callbacks, objects freed by their owner).
   - `src/gpu` (from main, feature `gpu`): byte views of plain f32/u64 buffers for wgpu uploads and read-back
     (allowed in `src/lib.rs`).

   Don't add `unsafe` anywhere else. External dependencies may carry their own isolated `unsafe`: the editor
   depends on PhotoCraft's `photocraft-tablet` (pinned, ADR 0007) for pen tablet input, whose macOS AppKit monitor
   is its one `unsafe` module; MaterialCraft itself adds none for tablets (`gui_paint::input::tablet` is safe code).
7. **Licence and attribution.** MaterialCraft is `MIT OR Apache-2.0` (`LICENSE-MIT`, `LICENSE-APACHE`, `NOTICE`).
   Code copied from another Craft app (PhotoCraft is MIT OR Apache-2.0) keeps its licence notice in the file and gets
   a row in `ATTRIBUTION.md` in the same commit; third-party crates are listed in `THIRD_PARTY_LICENSES.md`.
8. **Reach first (effects and tools).** Every filter, generator and procedural texture declares how far it reads
   around a texel before it is used: `FilterKernel::reach` and `GeneratorSource::stack_reach` have no default, so
   new code won't compile without one. Prove the declared reach with a test (see `src/filters/tests_reach.rs` and
   `src/generators/tests_reach.rs`) and add the effect to the reach ledger (`src/generators/reach_ledger.rs`); a
   test fails while a registered effect is missing there. A new native effect kind gets an arm in
   `EffectKind::reach` (the match has no wildcard). A new tool that changes pixels states which tiles and channels
   it touches, so only that part of the stack is recomposited. When an effect changes, verify its reach again and
   update its ledger date.
9. **Commits** explain why, build on their own, and keep mechanical changes (formatting, renames) apart from logic.
