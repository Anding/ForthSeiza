\ Verify Seiza loads shared formats without numerical or application policy.

NEED simple-tester
NEED ForthSeiza

Tstart
T{ [DEFINED] compute-imageStats [DEFINED] apply-displayFunction-to }T 0 0 ==
T{ [DEFINED] analyze-stars [DEFINED] start_computation }T 0 0 ==
T{ [DEFINED] pipeline-process }T 0 ==
T{ s" NAXIS1  = 640" FITS.read-line drop hashS -rot hashS swap }T s" 640" hashS s" NAXIS1" hashS ==
Tend
