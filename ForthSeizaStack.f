\ Independent native stacking; no solver binding or equipment operations.
NEED ForthBase
NEED buffers
NEED ForthVT100

LIBRARY: ForthSeizaBridge.dll
Extern: int "C" SeizaStackStart( char * executable, int length, int timeout-seconds ) ;
Extern: int "C" SeizaStackStop() ;
Extern: int "C" SeizaStackOpen( char * reference, int reference-length, char * config, int config-length ) ;
Extern: int "C" SeizaStackAdd( int handle, char * path, int length ) ;
Extern: int "C" SeizaStackPreview( int handle, char * directory, int length ) ;
Extern: int "C" SeizaStackSave( int handle, char * path, int length ) ;
Extern: int "C" SeizaStackState( int handle ) ;
Extern: int "C" SeizaStackClose( int handle ) ;
Extern: int "C" SeizaStackLastError( char * output, int capacity ) ;
Extern: int "C" SeizaStackLastReply( char * output, int capacity ) ;

s" E:\Coding\ForthSeiza\stacker\target\release\seiza-stacker.exe" $value seiza.stack.executable
s" " $value seiza.stack.config
300 value seiza.stack.timeout
0 value seiza.stack.started
8192 allocate-buffer constant seiza.stack.text

: .seiza.stack-error ( -- )
\ Print the last stack error; solver errors and state remain independent.
    seiza.stack.text BUFFER_ADDR 8192 SeizaStackLastError
    seiza.stack.text BUFFER_ADDR swap .E>
;

: .seiza.stack-result ( -- )
\ Print the last operation's JSON state or admission diagnostics.
    seiza.stack.text BUFFER_ADDR 8192 SeizaStackLastReply
    seiza.stack.text BUFFER_ADDR swap type cr
;

: seiza.stack.start ( -- flag )
\ Start once on demand; a broken worker is never automatically restarted.
    seiza.stack.started if true exit then
    seiza.stack.executable seiza.stack.timeout SeizaStackStart 1 =
    dup if true -> seiza.stack.started else .seiza.stack-error then
;

: seiza.stack.stop ( -- )
\ Explicitly stop the independent worker; every old handle becomes invalid.
    SeizaStackStop drop false -> seiza.stack.started
;

: seiza.stack.open ( caddr u -- handle|0 )
\ Open a completed reference using optional JSON config; reference is integrated.
    seiza.stack.start 0= if 2drop 0 exit then
    seiza.stack.config SeizaStackOpen
    dup 0< if drop .seiza.stack-error 0 then
;

: seiza.stack.add ( handle caddr u -- status )
\ Submit once: 1 accepted, 0 rejected, -1 operational/unknown-outcome error.
    SeizaStackAdd dup 0< if .seiza.stack-error then
;

: seiza.stack.preview ( handle directory-addr directory-u -- flag )
\ Publish an atomic BMP generation and latest.dat in the given stream directory.
    SeizaStackPreview 1 = dup 0= if .seiza.stack-error then
;

: seiza.stack.save ( handle caddr u -- flag )
\ Save a linear floating-point FITS/XISF without closing or clearing the stack.
    SeizaStackSave 1 = dup 0= if .seiza.stack-error then
;

: seiza.stack.state ( handle -- flag )
\ Query native counts/reference; inspect them with .seiza.stack-result.
    SeizaStackState 1 = dup 0= if .seiza.stack-error then
;

: seiza.stack.close ( handle -- flag )
\ Release an accumulator without implicit save; other handles remain open.
    SeizaStackClose 1 = dup 0= if .seiza.stack-error then
;
