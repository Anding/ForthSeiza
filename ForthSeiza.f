\ Compile-time Seiza plate-solver implementation for the shared solve-image API.

NEED ForthBase
NEED ForthAstroSolver
NEED ForthAstroPaths
NEED ordered-maps

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

: seiza.write-temp-FITSfilepath
    { img suffix-addr suffix-u filepath-buffer -- }
\ Build the pathname stem for Seiza's private solver image.
    img suffix-addr suffix-u filepath-buffer solver.temp-FITSfilepath if
        filepath-buffer reset-buffer cr ." Seiza filepath buffer full" cr
    then
;

: seiza.save-temp-FITS ( img -- )
\ Temporarily replace FITS pathname policy while writing the solver image.
    seiza.temp-FITSpath ['] seiza.write-temp-FITSfilepath solver.save-temp-FITS
;

: seiza.wcs-filepath ( caddr u -- caddr u )
    4 - $-> seiza.string0
    s" wcs" $+> seiza.string0
    seiza.string0
;

: seiza.solve-image { img | map status -- solved? }
    img FRAME_METADATA @ -> map
    img seiza.save-temp-FITS
    seiza.temp-FITSpath buffer_used 0= if img no-solver exit then
    seiza.start 0= if
        s" FAILED" map =>" SOLVSTAT"
        -1 exit
    then
    seiza.temp-FITSpath buffer-to-string
    s" RA" map map@
    s" Dec" map map@
    s" XPIXSZ" map map@
    s" FOCALLEN" map map@
    seiza.radius-deg
    seiza.scale-tolerance
    SeizaBridgeSolve -> status
    status 1 = if
        seiza.temp-FITSpath buffer-to-string seiza.wcs-filepath
            img solver.import-WCS if img no-solver exit then
        s" SEIZA" map solver.mark-solved
        0
    else
        s" FAILED" map =>" SOLVSTAT"
        -1
    then
;

ASSIGN seiza.solve-image TO-DO solve-image
