\ Verify the Seiza solver loads only the shared frame and FITS boundary.

NEED simple-tester
NEED ForthSeiza

[DEFINED] XISF.encode-header [IF]
    abort" ForthSeiza must not load the XISF codec"
[THEN]

[DEFINED] xisf.load-FITSfile [IF]
    abort" ForthSeiza must not load FITS readers"
[THEN]

Tstart
T{ s" NAXIS1  = 640" FITS.read-line drop hashS -rot hashS swap }T s" 640" hashS s" NAXIS1" hashS ==
Tend

bye
