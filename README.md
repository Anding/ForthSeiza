# ForthSeiza

Compile-time Seiza implementation of the shared Forth `solve-image`
contract. It is the default AstroImagingInForth astrometric solver.

## Architecture

`ForthSeizaBridge.dll` is a Rust DLL loaded by VFX Forth. It starts one live
`seiza worker` child, keeps its catalogues initialized, and exchanges
newline-delimited JSON-RPC with the child through standard input and output.
The DLL retries exactly once after a pipe or protocol failure.

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

It writes `<astro.working-root>\<UUID>\solve.fits`, performs a hinted solve, and
imports the generated FITS-card `.wcs` sidecar into the image context's ordered
FITS map. A solve records `SOLVER=SEIZA`, `SOLVSTAT=SOLVED`, WCS keys, and
`10UALPT`. A missing mount hint, failed solve, or bridge failure records only
`SOLVSTAT=FAILED`; the caller can still save science files.

The solver FITS save temporarily assigns Seiza's private builder to
`write-science-filepath`, then restores the previous default or user action
before returning or rethrowing a save error. Subsequent science XISF/FITS
payloads therefore retain the configured science destination.

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
