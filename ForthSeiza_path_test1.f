\ Seiza solver FITS path substitution and restoration.

NEED simple-tester
include "%libdir%\ForthAstroFormats\ForthAstroFormats_test_support.f"
NEED ForthSeiza

s" E:\images" $-> astro.root
0 value seiza.path-test.frame
0 value seiza.path-test.notifications

: seiza.path-test-solved ( metadata -- )
\ Count notifications without needing application or mount metadata.
    drop seiza.path-test.notifications 1+ -> seiza.path-test.notifications
;
ASSIGN seiza.path-test-solved TO-DO solver.solved-metadata

: seiza.path-test-FITS { frame suffix-addr suffix-u filepath-buffer -- }
\ Distinct caller policy used to prove Seiza restores the previous action.
    frame drop
    filepath-buffer reset-buffer
    s" E:\Coding\ForthSeiza\caller-science"
        filepath-buffer write-buffer drop
    suffix-addr suffix-u filepath-buffer write-buffer drop
;

ASSIGN seiza.path-test-FITS TO-DO write-filepath-fits
ACTION-OF write-filepath-fits constant seiza.path-test.saved-action

: seiza.path-test-failing-path ( frame suffix-addr suffix-u filepath-buffer -- )
\ Fail before opening a file to exercise restoration of the caller's action.
    -79 throw
;

: seiza.path-test-failure ( -- )
\ Invoke the shared writer with a deliberately failing pathname action.
    seiza.path-test.frame seiza.temp-FITSpath
        ['] seiza.path-test-failing-path solver.save-temp-FITS
;

test.make-frame -> seiza.path-test.frame
s" 11111111-2222-3333-4444-555555555555"
    seiza.path-test.frame FRAME_METADATA @ =>" UUID"

Tstart
T{ s" RA" seiza.path-test.frame FRAME_METADATA @ map@ }T 0 0 ==
T{ s" Dec" seiza.path-test.frame FRAME_METADATA @ map@ }T 0 0 ==
s" 67.25" seiza.path-test.frame FRAME_METADATA @ =>" RA"
T{ s" RA" seiza.path-test.frame FRAME_METADATA @ map@ s" 67.25" compare }T 0 ==
T{ seiza.path-test.frame seiza.save-temp-FITS }T ==
T{ seiza.temp-FITSpath buffer-to-string hashS
}T s" E:\images\working\11111111-2222-3333-4444-555555555555\solve.fits" hashS ==
T{ seiza.temp-FITSpath buffer-to-string FileExists? }T -1 ==
T{ ACTION-OF write-filepath-fits seiza.path-test.saved-action = }T -1 ==
T{ ' seiza.path-test-failure catch }T -79 ==
T{ ACTION-OF write-filepath-fits seiza.path-test.saved-action = }T -1 ==
T{ s" SEIZA" seiza.path-test.frame FRAME_METADATA @ solver.mark-solved }T ==
T{ s" SOLVER" seiza.path-test.frame FRAME_METADATA @ map@ s" SEIZA" compare }T 0 ==
T{ s" SOLVSTAT" seiza.path-test.frame FRAME_METADATA @ map@ s" SOLVED" compare }T 0 ==
T{ seiza.path-test.notifications }T 1 ==
Tend

seiza.temp-FITSpath buffer-to-string delete-file drop
seiza.path-test.frame free-frame
