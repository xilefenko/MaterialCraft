# materialcraft-substance: the Substance 3D Painter importer

Imports `.spp` projects into MaterialCraft paint documents. Reading the file is `spp-reader`'s job
(`crates/spp-reader`); this crate turns what it reads into MaterialCraft's layers, and says exactly what it can't.

Painter projects lean on hundreds of compiled Substance graphs (generators, procedural fills, filters) and on brush
types and alphas that only Painter can evaluate. No one person can rewrite them all. The importer is therefore built
so that it **never pretends**: every layer it can't reproduce is kept, locked and labelled with what blocks it, the
document still looks exactly like Painter's, and each piece of support someone adds unlocks layers in every project
that uses it.

## How an import comes out

For each texture set, top to bottom:

| Layer | What it is |
|---|---|
| Painter result (reference) | Painter's flattened result, hidden, for comparison. |
| 🔒 Painter correction | Painter's pixels only on the texels the layers below don't reproduce. Switch it off to see what the layers do on their own. How much it covers, per channel, is how far the set is from fully editable. |
| Imported layers | Painter's layer tree: names, folders, visibility, blending, masks, effects. Layers that can be computed are ordinary editable layers. Layers that can't are **locked** (🔒): kept and toggleable, but their content can't be edited until what blocks them is supported (or the user unlocks them anyway). |
| Painter defaults | Painter's channel defaults, where nothing else covers. |

The final look is Painter's in every case; what changes as support grows is how much of it comes from real,
editable layers instead of the correction.

## Blockers and the support catalog

Every part of a layer that can't be computed names a **blocker** with a stable key (`substance:mg_dirt`,
`brush-alpha:charcoal_brush`, `strokes:painted-mask`, … — the vocabulary is in `spp-reader`'s `blockers`). Locks
store these keys in the document, so a saved `.mcpaint` knows what would unlock each of its layers.

`translate::catalog` answers, for every key, what MaterialCraft can do about it today:

| Status | Meaning | What unlocks it |
|---|---|---|
| Replaceable | MaterialCraft has its own stand-in | choosing "replace" in the import review |
| Replayable | MaterialCraft's brush can paint the strokes again | importing with stroke replay on |
| Not supported yet | nothing yet | adding support (below) |

The editor's Blockers panel, the lock tooltips, the import report and `mcsub blockers` all read the catalog, so
support added in one place shows up everywhere.

## Finding what to work on

```sh
cargo run --release -p materialcraft-substance --bin mcsub -- blockers <folder of .spp> --projects > blockers.md
```

ranks every blocker over a folder of projects by how many layers it locks **alone**: the layers that become editable
as soon as it is supported. Run it on your own projects; nothing from them belongs in the repository.

## Adding support for a Substance procedural

1. **Write the effect** in MaterialCraft: a generator (`src/generators`), a procedural texture (`src/proc_textures`)
   or a filter (`src/filters`), from scratch, from what the procedural visibly does. Declare its reach and prove it
   with a test (see the repository's `AGENTS.md`, rule 8).
2. **Map it** in `translate/replacements`: one `Replacement` entry in `table.rs` (mask generators), `textures.rs`
   (procedural fills and textures) or `filters.rs`. It lists Painter's graph names, our id, a note on what is left
   out, and a function translating Painter's parameter values into ours.
3. **Measure it** against Painter's saved results on your own projects:
   `cargo run --release -p materialcraft-parity --bin mcparity -- run generator.<our id> --corpus <folder>`.
   The import review shows the measured accuracy next to the choice.
4. **Check the unlock**: `mcsub blockers` should list the key as Replaceable, and
   `mcsub reimport <old.mcpaint> <new.mcpaint> --procedurals replace` prints which layers it unlocked.

Brush support works the same way through the stroke replay (`translate/layers/replay`): a brush alpha or brush type
the replay learns to paint stops being a blocker.

## Commands

`mcsub` (in `src/bin/mcsub.rs`, with the full option list in its header):

| Command | What it does |
|---|---|
| `convert <in.spp> <out.mcpaint> [--procedurals replace] [--strokes replay]` | import and save |
| `reimport <in.mcpaint> <out.mcpaint> …` | import a converted document's `.spp` again with today's support and print what got unlocked |
| `blockers <folder>` | rank what keeps layers locked over a folder of projects |
| `report <in>` | the import report, with one line per blocker |
| `analyze <file.spp>` | the Substance procedurals a project uses and which can be replaced |
| `maps <in> <dir>` | the final maps as PNGs |
| `inspect`, `tree`, `doc`, `model` | look inside a project |

## Layout

| Module | What |
|---|---|
| `translate::importer` | the import pipeline: mesh, texture sets, baked maps, cameras, layer stacks, the report |
| `translate::layers` | Painter's layer stacks → ours: content, masks, blending, projections, polygon fills, anchors, stroke records and replay, what is computable (`computability`, `verdict`), the arrangement (`strategy`) and the correction layer (`correction`) |
| `translate::replacements` | the replacement table and the user's per-procedural plan |
| `translate::catalog`, `translate::blocker_summary` | the support catalog, and a document's locks grouped by blocker |
| `translate::mesh` | the reader's scene mesh → MaterialCraft's mesh |
| `tools` | what `mcsub` runs: conversion, re-import and lock diffs, the blocker census, comparisons and the measuring tools |
