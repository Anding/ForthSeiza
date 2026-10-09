\ Seiza solver FITS path substitution and restoration.

NEED simple-tester
include "%libdir%\ForthAstroFormats\ForthAstroFormats_test_support.f"
NEED ForthSeiza

0 value seiza.path-test.frame

: seiza.path-test-science { frame filepath-buffer -- }
\ Distinct caller policy used to prove Seiza restores the previous action.
    frame drop
    filepath-buffer reset-buffer
    s" E:\Coding\ForthSeiza\caller-science"
        filepath-buffer write-buffer drop
;

ASSIGN seiza.path-test-science TO-DO write-science-filepath
ACTION-OF write-science-filepath constant seiza.path-test.saved-action

test.make-frame -> seiza.path-test.frame
s" 11111111-2222-3333-4444-555555555555"
    seiza.path-test.frame FRAME_METADATA @ =>" UUID"

Tstart
T{ seiza.path-test.frame seiza.save-temp-FITS }T ==
T{ seiza.temp-FITSpath buffer-to-string hashS
}T s" E:\images\working\11111111-2222-3333-4444-555555555555\solve.fits" hashS ==
T{ seiza.temp-FITSpath buffer-to-string FileExists? }T -1 ==
T{ ACTION-OF write-science-filepath seiza.path-test.saved-action = }T -1 ==
Tend

seiza.temp-FITSpath buffer-to-string delete-file drop
seiza.path-test.frame free-frame
