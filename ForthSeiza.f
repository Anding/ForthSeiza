\ Compile-time Seiza plate-solver implementation for the shared solve-image API.

NEED ForthBase
NEED ForthAstroSolver

LIBRARY: ForthSeizaBridge.dll
Extern: int "C" SeizaBridgeStart(
    char * executable, int executable-length,
    char * catalog, int catalog-length,
    char * index, int index-length
) ;
Extern: int "C" SeizaBridgeSolve(
    char * image-path, int image-path-length,
    char * ra-deg, int ra-deg-length,
    char * dec-deg, int dec-deg-length,
    char * pixel-size-um, int pixel-size-um-length,
    char * focal-length-mm, int focal-length-mm-length,
    char * radius-deg, int radius-deg-length,
    char * scale-tolerance, int scale-tolerance-length
) ;
Extern: int "C" SeizaBridgeStop() ;
Extern: int "C" SeizaBridgeLastError( char * output, int capacity ) ;

s" E:\Coding\seiza\target\release\seiza.exe" $value seiza.executable
s" E:\seiza-data" $value seiza.catalog
s" " $value seiza.index
s" 2.0" $value seiza.radius-deg
s" 0.2" $value seiza.scale-tolerance
s" " $value seiza.string0

0 value seiza.started

FILEPATH_SIZE allocate-buffer constant seiza.temp-FITSpath

: seiza.start ( -- started? )
    seiza.started if -1 exit then
    seiza.executable seiza.catalog seiza.index SeizaBridgeStart
    dup 1 = if
        drop -1 -> seiza.started -1
    else
        drop 0
    then
;

: seiza.stop ( -- )
    seiza.started if SeizaBridgeStop drop 0 -> seiza.started then
;

: seiza.temp-FITSfilepath { img | filepath-buffer -- filepath-buffer }
    seiza.temp-FITSpath -> filepath-buffer
    filepath-buffer reset-buffer
    s" E:\images\working\" filepath-buffer write-buffer drop
    s" UUID" img FRAME_METADATA @ >string filepath-buffer write-buffer drop
    '\' filepath-buffer echo-buffer drop
    filepath-buffer buffer-punctuate-filepath
    s" solve.fits" filepath-buffer write-buffer drop
    filepath-buffer
;

: seiza.wcs-filepath ( caddr u -- caddr u )
    4 - $-> seiza.string0
    s" wcs" $+> seiza.string0
    seiza.string0
;

: seiza.optional-hint { key-addr key-u map -- value-addr value-u }
\ Pointing hints are absent when imaging without a mount. Empty RA and Dec
\ make the hinted solver fail cleanly rather than aborting metadata lookup.
    key-addr key-u map item? if
        drop
        key-addr key-u map >string
    else
        drop
        0 0
    then
;

: seiza.solve-image { img | filepath-buffer map status -- solved? }
    img FRAME_METADATA @ -> map
    img seiza.temp-FITSfilepath -> filepath-buffer
    img filepath-buffer save-FITSimage-to
    seiza.start 0= if
        s" FAILED" map =>" SOLVSTAT"
        -1 exit
    then
    filepath-buffer buffer-to-string
    s" RA" map seiza.optional-hint
    s" Dec" map seiza.optional-hint
    s" XPIXSZ" map >string
    s" FOCALLEN" map >string
    seiza.radius-deg
    seiza.scale-tolerance
    SeizaBridgeSolve -> status
    status 1 = if
        filepath-buffer buffer-to-string seiza.wcs-filepath
            img solver.import-WCS
        s" SEIZA" map =>" SOLVER"
        s" SOLVED" map =>" SOLVSTAT"
        0
    else
        s" FAILED" map =>" SOLVSTAT"
        -1
    then
;

ASSIGN seiza.solve-image TO-DO solve-image
