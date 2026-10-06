# ForthSeiza

Compile-time Seiza implementation of the shared Forth `solve-image`
contract. It replaces ForthASTAP by being loaded before
AstroImagingInForth, not through a runtime solver adapter.

## Architecture

`ForthSeizaBridge.dll` is a Rust DLL loaded by VFX Forth. It starts one live
`seiza worker` child, keeps its catalogues initialized, and exchanges
newline-delimited JSON-RPC with the child through standard input and output.
The DLL retries exactly once after a pipe or protocol failure.

## Compile-time selection

Start a fresh VFXterm session, load ForthSeiza, configure it, then load the
AstroImagingInForth integration script:

```forth
NEED ForthSeiza
s" E:\Coding\seiza\target\release\seiza.exe" $-> seiza.executable
s" E:\seiza-data" $-> seiza.catalog
include scripts\AstroImagingInForth.f
```

AstroImagingInForth keeps this `solve-image` implementation and therefore
does not load ForthASTAP. To return to ASTAP, restart VFXterm and load
`scripts\AstroImagingInForth.f` without first loading ForthSeiza. The
selection is compile-time; do not load both packages in one session.

The public Forth implementation is:

```forth
seiza.solve-image ( img -- solved? )
```

It writes `E:\images\working\<UUID>\solve.fits`, performs a hinted solve, and
imports the generated FITS-card `.wcs` sidecar into the image context's ordered
FITS map. A solve records `SOLVER=SEIZA`, `SOLVSTAT=SOLVED`, WCS keys, and
`10UALPT`. A missing mount hint, failed solve, or bridge failure records only
`SOLVSTAT=FAILED`; the caller can still save science files.

## Hints

The image context remains the only source of the solve hint:

| FITS key | Use |
| --- | --- |
| `RA`, `Dec` | J2000 mount position in degrees |
| `XPIXSZ` | Camera pixel size in micrometres |
| `FOCALLEN` | Optical focal length in millimetres |

The bridge calculates scale as `206.265 * XPIXSZ / FOCALLEN`. It uses the
configured `seiza.radius-deg` and `seiza.scale-tolerance`; defaults are `2.0`
degrees and `0.2`.

## Configuration

Set the executable and local catalogue paths before the first solve:

```forth
s" E:\Coding\seiza\target\release\seiza.exe" $-> seiza.executable
s" E:\seiza-data" $-> seiza.catalog
```

`seiza.catalog` is required and is passed to Seiza as its `--data` directory.
`seiza.index` is optional and left empty by default because hinted-only
operation does not use blind solving.

Build the DLL for the same architecture as VFX Forth and place
`ForthSeizaBridge.dll` beside `VFXterm.exe` before loading `ForthSeiza.f`.
