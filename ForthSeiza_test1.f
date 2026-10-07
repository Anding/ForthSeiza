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
T{ s" NOHINT" seiza.test-image FRAME_METADATA @ seiza.optional-hint }T 0 0 ==
s" 04:29:05.0" seiza.test-image FRAME_METADATA @ =>" RA"
T{ s" RA" seiza.test-image FRAME_METADATA @ seiza.optional-hint nip }T 10 ==

Tend

seiza.test-image free-frame
bye
