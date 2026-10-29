\ Executable offline tutorial: generate fixtures with the ignored Rust fixture test.
\ Requires reference.fits and second.xisf in E:\images\tests\live-stack\inputs.
\ No equipment or plate-solving catalogues are used.
NEED ForthSeizaStack
NEED simple-tester

0 value stack.test-handle
0 value stack.test-old-handle

Tstart
T{ s" E:\images\tests\live-stack\inputs\reference.fits" seiza.stack.open
    -> stack.test-handle }T ==
T{ stack.test-handle 0> }T true ==
T{ stack.test-handle seiza.stack.state }T true ==
.seiza.stack-result
\ Open has already integrated the first light; submit only later completed files.
T{ stack.test-handle s" E:\images\tests\live-stack\inputs\second.xisf"
    seiza.stack.add }T 1 ==
.seiza.stack-result
T{ stack.test-handle s" E:\images\tests\live-stack\working\stack"
    seiza.stack.preview }T true ==
T{ stack.test-handle s" E:\images\tests\live-stack\linear.fits"
    seiza.stack.save }T true ==
T{ stack.test-handle s" E:\images\tests\live-stack\linear.xisf"
    seiza.stack.save }T true ==
\ Native path tracking refuses a duplicate input and source overwrite.
T{ stack.test-handle s" E:\images\tests\live-stack\inputs\reference.fits"
    seiza.stack.add }T -1 ==
T{ stack.test-handle s" E:\images\tests\live-stack\inputs\reference.fits"
    seiza.stack.save }T false ==
T{ stack.test-handle seiza.stack.close }T true ==
T{ stack.test-handle seiza.stack.state }T false ==
\ An explicit restart never makes an old handle valid for a new accumulator.
stack.test-handle -> stack.test-old-handle
T{ seiza.stack.stop }T ==
T{ s" E:\images\tests\live-stack\inputs\reference.fits" seiza.stack.open
    -> stack.test-handle }T ==
T{ stack.test-handle stack.test-old-handle <> }T true ==
T{ stack.test-old-handle seiza.stack.state }T false ==
T{ stack.test-handle seiza.stack.close }T true ==
T{ seiza.stack.stop }T ==
Tend
