\ Regression coverage for the ForthSeiza compile-time solver vocabulary.

NEED ForthSeiza
NEED simple-tester

4 3 1 allocate-frame constant seiza.test-image

Tstart

T{ seiza.catalog hashS }T s" E:\seiza-data" hashS ==
T{ s" E:\images\tests\astap\known-good\LUM-E8-F5100-12365844e78a.wcs"
   seiza.test-image solver.import-WCS
}T ==
T{ s" CRVAL1" seiza.test-image FRAME_METADATA @ >string nip 0> }T -1 ==
T{ s" CTYPE1" seiza.test-image FRAME_METADATA @ >string drop 8 hashS }T s" RA---TAN" hashS ==
T{ seiza.start }T -1 ==

Tend

seiza.stop
seiza.test-image free-frame
bye
