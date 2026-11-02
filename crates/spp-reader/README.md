# spp-reader

Reads Substance 3D Painter projects (`.spp`) into plain Rust data: the file container, Painter's records and its
layer model. It knows no application. MaterialCraft's importer (`crates/substance`) is built on it, and any other
painter can build its own importer on the same reader.

The only dependency is `lz4_flex`. The crate contains no Painter code, data or resources: everything is read from
the user's own files at run time, from what those files contain.

## What it reads

| Module | What |
|---|---|
| `container` | The generic formats: HDF5 (the `.spp` itself), Qt QDataStream, byte cursors. Knows nothing of Painter. |
| `decode` | Painter's records: project settings, the resource table and images, the texture cache (the flattened channels Painter saved), the mesh scene (`scene.assbin`, decoded into `SceneMesh`), mesh metadata and normalisation, cameras, and `document.bin` as a raw object tree. |
| `model` | Painter's concepts built from `document.bin`: texture sets, channels, the layer tree (fill, paint, folder), per-channel blending, masks, effects (fills, filters, generators, levels, colour selection, anchors, polygon fills), sources (uniform, bitmap, procedural, reference), brushes and strokes, bake settings. Unknown classes are kept raw and counted, never dropped. |
| `project` | One opened `.spp` with typed access to its parts. |
| `inspect` | Human-readable reports about a project or a folder of them. |
| `blockers` | The shared vocabulary for what an importer can't evaluate yet (see below). |

## Example

```sh
cargo run -p spp-reader --example layers -- path/to/project.spp
```

```rust
use spp_reader::decode::assbin::AssbinGeometry;
use spp_reader::project::{SppProject, paths};

let project = SppProject::open(path)?;
let mesh = AssbinGeometry::parse(&project.read(paths::SCENE)?)?; // world-space parts, glTF UVs
let (document, report) = project.model()?;                        // texture sets and their layer trees
for set in &document.texture_sets {
    println!("{}: {} layers", set.name, set.layers.len());
}
```

See `examples/layers.rs` for the whole program.

## Blockers: a shared vocabulary

Painter projects are full of compiled Substance graphs (generators, procedural fills, filters), brush alphas and
brush types that an importer can't evaluate until someone writes an equivalent. `blockers` gives each one a stable
key, so every importer, report and corpus census names the same thing the same way:

| Key | What blocks |
|---|---|
| `substance:<graph>` | a compiled Substance graph, e.g. `substance:mg_dirt` |
| `brush-alpha:<name>` | strokes painted with a brush alpha the importer doesn't have |
| `brush:<what>` | strokes the importer's brush can't paint (particles, 3D paths, stencils, projection …) |
| `strokes:<what>` | strokes that were not replayed |
| `feature:<id>` | a Painter feature the importer doesn't model yet |
| `class:<class>` | a layer, effect or source class the reader doesn't know |

What an importer can *do* about a key (replace it, replay it, or keep the layer locked) is the importer's own
business; MaterialCraft's answer is its support catalog (`crates/substance/src/translate/catalog.rs`).

## Robustness

The reader parses untrusted files. Malformed input is reported as an error (`ProjectError`, `DecodeError`); unknown
classes, fields and newer formats are kept raw and reported rather than guessed.

## Tests

`cargo test -p spp-reader` runs everything that needs no Painter files. The corpus tests run over real projects
when given a folder: `MC_SUBSTANCE_CORPUS=<folder> cargo test -p spp-reader` (they skip without it). Nothing read
from a project is ever committed.

## Licence

MIT OR Apache-2.0, like the rest of MaterialCraft.
