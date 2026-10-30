# ForthSeiza

Compile-time Seiza implementation of the shared Forth `solve-image`
contract. It is the default AstroImagingInForth astrometric solver.

## Architecture

`ForthSeizaBridge.dll` is a Rust DLL loaded by VFX Forth. It starts one live
`seiza worker` child, keeps its catalogues initialized, and exchanges
newline-delimited JSON-RPC with the child through standard input and output.
The solver transport retries exactly once after a pipe or protocol failure.
The independent stacking transport below never replays requests.

## Compile-time selection

The ordinary home session loads Seiza automatically:

```forth
include scripts\HomeObservatory.f
```

To select ASTAP astrometry instead, restart VFXterm, load the complete
`ForthASTAP` package first, and then load the home session. The focus-only
`ForthASTAPFocus` package is compatible with Seiza because it does not define
`solve-image`.

The public Forth implementation is:

```forth
seiza.solve-image ( img -- solved? )
```

It writes `<astro.root>\working\<UUID>\solve.fits`, performs a hinted solve, and
imports the generated FITS-card `.wcs` sidecar into the image context's ordered
FITS map. A solve records `SOLVER=SEIZA`, `SOLVSTAT=SOLVED`, WCS keys, and calls
`solver.solved-metadata`. AstroImagingInForth binds this to generate `10UALPT`
for every successful frame solve, not just model building.
A missing mount hint, failed solve, or bridge failure records only
`SOLVSTAT=FAILED`; the caller can still save science files.

The solver FITS save executes Seiza's private filename creator without changing
`write-filepath-fits`. Ordinary save errors are reported, clear the temporary
path, and skip solving. Later science destinations are unchanged.

## Hints

The image context remains the only source of the solve hint:

Hints use ForthBase ordered string-map `map@` lookups. Missing or empty
metadata supplies `( 0 0 )` to the bridge rather than aborting the lookup.

| FITS key | Use |
| --- | --- |
| `RA`, `Dec` | J2000 mount position in degrees |
| `XPIXSZ` | Camera pixel size in micrometres |
| `FOCALLEN` | Optical focal length in millimetres |

The bridge calculates scale as `206.265 * XPIXSZ / FOCALLEN`. It uses the
configured `seiza.radius-deg` and `seiza.scale-tolerance`; defaults are `2.0`
degrees and `0.2`.

## Configuration

The default installation paths are:

```forth
s" E:\Coding\seiza\target\release\seiza.exe" $-> seiza.executable
s" E:\seiza-data" $-> seiza.catalog
```

Override either value before the first solve when Seiza is installed
elsewhere. `seiza.catalog` is required and is passed to Seiza as its `--data`
directory.
`seiza.index` is optional and left empty by default because hinted-only
operation does not use blind solving.

Build the DLL for the same architecture as VFX Forth and place
`ForthSeizaBridge.dll` beside `VFXterm.exe` before loading `ForthSeiza.f`.

## Live stacking

`NEED ForthSeizaStack` loads stacking without binding `solve-image` or starting
equipment. The DLL manages a separate **64-bit** `seiza-stacker.exe` child.
Pixels, calibration masters and accumulators stay in that child; only paths,
opaque handles and small JSON results cross the 32-bit boundary.

Build from sibling ForthSeiza and Seiza checkouts:

```powershell
cargo build --locked --release --target i686-pc-windows-msvc
cargo build --manifest-path stacker\Cargo.toml --locked --release --target x86_64-pc-windows-msvc
```

Deploy the DLL beside the **actual** `VFXterm.exe` being launched, closing hosts
that use that copy first. With the explicit worker target above, set:

```forth
s" E:\Coding\ForthSeiza\stacker\target\x86_64-pc-windows-msvc\release\seiza-stacker.exe" $-> seiza.stack.executable
```

Alternatively, on a 64-bit Rust host, build the worker without `--target` to
use its default executable path. The default response timeout is 300 seconds;
set `seiza.stack.timeout` before starting (1-3600 seconds).

| Word | Stack effect | Meaning |
| --- | --- | --- |
| `seiza.stack.open` | `( caddr u -- handle\|0 )` | Integrate the completed reference once |
| `seiza.stack.open-channel` | `( alignment-handle caddr u -- handle\|0 )` | Open an **empty** filter stack on an existing alignment grid |
| `seiza.stack.add` | `( handle caddr u -- status )` | 1 accepted, 0 rejected, -1 error |
| `seiza.stack.state` | `( handle -- flag )` | Query counts and reference |
| `seiza.stack.preview` | `( handle directory-addr directory-u -- flag )` | Publish an 8-bit stretched BMP and `latest.dat` |
| `seiza.stack.save` | `( handle caddr u -- flag )` | Save linear F32 FITS/XISF; keep accumulating |
| `seiza.stack.close` | `( handle -- flag )` | Release without implicit save |
| `seiza.stack.stop` | `( -- )` | Stop only the stack child; invalidate handles |
| `seiza.stack.colour-preview` | `( handles count mode-addr mode-u directory-addr directory-u -- flag )` | Compose current channel means and atomically publish a BMP |
| `seiza.stack.colour-save` | `( handles count mode-addr mode-u path-addr path-u -- flag )` | Save native F32 colour FITS/XISF with transfer metadata |

`.seiza.stack-result` prints the last successful response, including native
admission diagnostics or a rejection reason. `.seiza.stack-error` prints the
last error. Ordinary Open has already integrated its reference: never Add it
again. **Open-channel starts empty:** explicitly Add its channel reference once.
That reference controls the channel's own normalization/noise weighting; the
alignment handle supplies only the common pixel grid and WCS.

Optional `seiza.stack.config` names a JSON file containing `bias`, `dark`,
`flat`, `dark_exposure_seconds`, and/or `options`. Master paths are absolute;
`options` is native Seiza `StackOptions` JSON, with native defaults and validation.
It must not contain `reference`, `handle` or `alignment_handle`.
Leave the filename empty for defaults.
For example:

```json
{
  "dark": "E:\\masters\\dark.xisf",
  "flat": "E:\\masters\\flat.fits",
  "dark_exposure_seconds": 120,
  "options": { "bin_factor": 2 }
}
```

The worker also accepts newline-delimited JSON-RPC 2.0 directly, with matching
IDs. Methods are `initialize` (`protocolVersion: 1`), `open`, `add`, `state`,
`preview`, `save`, `colour-preview`, `colour-save`, `close`, and `shutdown`.
`open` accepts the configuration fields above plus `reference` and optional
`alignment_handle`; the single-stack operations require `handle`.
Add/Save take `path`; Preview takes `directory` and optional `max_dimension`
(default 2048, maximum 8192). Stdout contains responses only.

Previews use Seiza's native physical-domain normalization and Auto-MTF stretch.
Each immutable UUID generation is complete before atomic replacement of
`latest.dat`. Both image keys select the same stretched BMP; raw linear data
is obtained with Save. No histogram/stack metadata stream is published yet.
Old preview generations are retained for viewers in Hold and need operator cleanup.
Science inputs and masters are protected against output overwrite.

A lost pipe, response-ID mismatch or timeout stops the child and reports
**unknown outcome**, without restart or replay. Previously integrated data
cannot be recovered from RAM after that failure. Explicitly stop before starting
again; old handle tokens cannot address new accumulators. **Checkpoint/resume
remains TBD.**

### Colour composition

Pass an array of 32-bit handle cells, its count, a native mode and destination.
All channels must be populated mono stacks on the same worker alignment grid.
Independent stacks are not implicitly registered merely because their dimensions
match. Channel exposure counts may differ; composition does not change them.

| Mode | Handle order | Native operation |
| --- | --- | --- |
| `RGB` | red, green, blue | RGB or super RGB |
| `LRGB` | luminance, red, green, blue | LRGB or super LRGB |
| `SHO`, `SOH`, `HSO`, `HOS`, `OSH`, `OHS`, `FORAXX-SHO` | ha, oiii, sii | Native narrowband palette |
| `HOO`, `FORAXX-HOO` | ha, oiii | Native narrowband palette |
| `MATRIX` | ha, oiii; sii only if used by the matrix | Native narrowband matrix |

`seiza.colour.config` optionally names a JSON file of native composition
options (not stack calibration). Names follow Seiza's C-facing colour settings:
`normalization` (`percentile`/`none`), `blackPercentile`, `whitePercentile`,
`normalizationSamples`, `crop` (`none`/`bounds`/`inscribed`), `luminanceMode`
(`native`/`super`), `luminanceWeight` (LRGB), `foraxxTargetMedian`,
`foraxxShadowsClip`, and `matrix`. Defaults are native Seiza defaults.
Matrix `red`, `green` and `blue` objects contain `ha`, `oiii`, `sii` weights;
omitted weights are zero. Settings inappropriate to the selected family fail.

For example, `{"luminanceMode":"super","crop":"inscribed"}` selects native
super-luminance and common covered pixels for RGB/LRGB. This is not a custom
mixing language. All stack inputs are linear; Foraxx produces `DISPLAY`
samples, saved with their native metadata and previewed **without another
normalization/stretch**. Ordinary compositions remain `LINEAR`.
Uncovered samples remain NaN in saved data and appear black in previews.

The direct worker colour request uses `mode`, an explicit `channels` role-to-handle
object, optional `options`, and either `directory` (preview) or `path` (save).
Preview also accepts `max_dimension`. For example:

```json
{"mode":"RGB","channels":{"red":1,"green":2,"blue":3},"options":{"crop":"bounds"},"path":"E:\\stacks\\rgb.xisf"}
```

Missing/stale/empty channels, wrong roles, incompatible grids and source/master
overwrite are refused. Individual mono previews remain independently available.

### Offline checks

```powershell
cargo test
cargo test --manifest-path stacker\Cargo.toml
$env:SEIZA_STACKER_FIXTURES = 'E:\images\tests\live-stack\inputs'
cargo test --manifest-path stacker\Cargo.toml export_offline_fixtures -- --ignored
```

With the bridge and worker deployed, `ForthSeizaStack_test.f` is a commented
executable tutorial for a dedicated offline VFX session. It exercises real
Open/Add/State/Preview/Save/Close, duplicate refusal, restart invalidation,
shared-grid colour publication/export and stale colour handles, then stops its
worker. No camera, mount or solving catalogues are needed.
