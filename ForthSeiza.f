\ Compile-time Seiza plate-solver implementation for the shared solve-image API.

NEED ForthBase
NEED FiniteFractions
NEED Forth-map
NEED AstroCalc
NEED ForthXISF

LIBRARY: ForthSeizaBridge.dll
Extern: int "C" SeizaBridgeStart( char * int char * int char * int ) ;
Extern: int "C" SeizaBridgeSolve( char * int char * int char * int char * int char * int char * int char * int ) ;
Extern: int "C" SeizaBridgeStop() ;

s" E:\Coding\seiza\target\release\seiza.exe" $value seiza.executable
s" " $value seiza.catalog
s" " $value seiza.index
s" 2.0" $value seiza.radius-deg
s" 0.2" $value seiza.scale-tolerance
s" " $value seiza.string0

0 value seiza.started
0 value seiza.solved.RA
0 value seiza.solved.Dec
0 value seiza.reported.RA
0 value seiza.reported.Dec
0 value seiza.reported.Sidereal
0 value seiza.reported.NightOf
s" " $value seiza.reported.Pierside$

256 buffer: seiza.wcs-line
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
    s" UUID" img FITS_MAP @ >string filepath-buffer write-buffer drop
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

 : seiza.import-WCS { caddr u img | map fileid -- }
\ Merge Seiza's CRLF-terminated WCS cards into the image context FITS map.
    img FITS_MAP @ -> map
    caddr u r/o open-file abort" Cannot open Seiza WCS file" -> fileid
    begin
        seiza.wcs-line 255 fileid read-line abort" Cannot read Seiza WCS file"
    while
        seiza.wcs-line swap XISF.read-FITSline
        dup 0= if
            drop map =>
        else
            drop
        then
    repeat
    fileid close-file abort" Cannot close Seiza WCS file"
;

: seiza.~Dec$ ( deg-min-sec -- caddr u)
    ':' ':' -1 ~custom$
;

: seiza.~RA$ ( hr-min-sec -- caddr u)
    ':' ':' 0 ~custom$
    s" HH:MM:SS.0" drop dup >R
    swap move R> 10
;

: seiza.prepare-alpt { map -- }
    s" OBJCTRA" map >string >number~ -> seiza.reported.RA
    s" OBJCTDEC" map >string >number~ -> seiza.reported.Dec
    s" SIDEREAL" map >string >number~ -> seiza.reported.Sidereal
    s" NIGHTOF" map >string >number~ -> seiza.reported.NightOf
    s" PIERSIDE" map >string $-> seiza.reported.Pierside$
    s" CRVAL1" map >string >float drop 1.5E1 f/ fp~ -> seiza.solved.RA
    s" CRVAL2" map >string >float drop fp~ -> seiza.solved.Dec
;

: seiza.formatALPT ( -- caddr u)
    s\" s\" " $-> seiza.string0
    seiza.reported.RA seiza.reported.Dec seiza.reported.NightOf JNOW swap
    seiza.~RA$ $+> seiza.string0 s" ," $+> seiza.string0
    seiza.~Dec$ $+> seiza.string0 s" ," $+> seiza.string0
    seiza.reported.Pierside$ $+> seiza.string0 s" ," $+> seiza.string0
    seiza.solved.RA seiza.solved.Dec seiza.reported.NightOf JNOW swap
    seiza.~RA$ $+> seiza.string0 s" ," $+> seiza.string0
    seiza.~Dec$ $+> seiza.string0 s" ," $+> seiza.string0
    seiza.reported.Sidereal seiza.~RA$ $+> seiza.string0
    s\" \" add-alignment-point" $+> seiza.string0
    seiza.string0
;

: seiza.solve-image { img | filepath-buffer map status -- solved? }
    img FITS_MAP @ -> map
    img seiza.temp-FITSfilepath -> filepath-buffer
    img filepath-buffer save-FITSimage-to
    seiza.start 0= if
        s" FAILED" map =>" SOLVSTAT"
        -1 exit
    then
    filepath-buffer buffer-to-string
    s" RA" map >string
    s" Dec" map >string
    s" XPIXSZ" map >string
    s" FOCALLEN" map >string
    seiza.radius-deg
    seiza.scale-tolerance
    SeizaBridgeSolve -> status
    status 1 = if
        filepath-buffer buffer-to-string seiza.wcs-filepath img seiza.import-WCS
        s" SEIZA" map =>" SOLVER"
        s" SOLVED" map =>" SOLVSTAT"
        map seiza.prepare-alpt
        seiza.formatALPT map =>" 10UALPT"
        0
    else
        s" FAILED" map =>" SOLVSTAT"
        -1
    then
;

[UNDEFINED] solve-image [IF]
DEFER solve-image ( img -- solved? )
[THEN]
ASSIGN seiza.solve-image TO-DO solve-image
