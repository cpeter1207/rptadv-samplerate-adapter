# rptadv-samplerate-adapter

`rptadv-samplerate-adapter` is the versioned dynamic sample-rate conversion
adapter for `rpt_advanced`. It implements the narrow descriptor specified by
ADR 0022 so controller and ring code have no direct external resampler ABI
dependency.

The adapter prepares persistent mono libswresample converters and exchanges
normalized F32 PCM (`-1.0` through `+1.0`). Conversion uses the SWR engine,
filter size 16, cutoff 1.0, and a Kaiser window. Forced resampling supports
soft clock compensation even when nominal input/output rates match. Each
process ratio stays within 1000 ppm of the fixed nominal ratio.

ABI2/SONAME2 replaces ABI1: callers supply nominal rates and maximum input/output
frame counts when creating the converter; the obsolete quality selectors are
removed. Consumers must update their descriptor, minimum version, and linkage
together. There is no ABI1 fallback. The adapter dynamically links system
`libswresample` and `libavutil`, without bundling or static-linking FFmpeg.

Setup prepares the filter and both initial and established-history buffering
paths. Processing accepts a bounded input prefix, retaining unconsumed input
with the caller. Zero-input calls drain available output without an EOF flush.
Reset replaces prior burst history using preallocated silence and retains
fractional phase; it does not recreate the converter. These operations remain
allocation-free, lock-free, nonblocking, and log-free after setup.

The queue query reports extra input backlog beyond intrinsic FIR lookahead.
The immutable output-delay query reports the warmed nominal delay, rounded to
the nearest output frame, for finite-media prefix trimming. Initial/reset
history is zero-extended. Neither query includes PLC. Real-time callers retain
the filter's intrinsic delay; finite rendering supplies enough trailing input
to retain its requested duration after trimming the reported prefix.

The public descriptor is selected by the stable
`RPTADV_SAMPLERATE_ADAPTER_CAPABILITY` macro. Consumers verify that name, the
ABI version, the descriptor's minimum readable prefix, and every required
function pointer before calling it. The header also rejects C compilation
modes that use short enum ABI values, which would be incompatible with this
adapter's C-int descriptor ABI.

Build a local shared object with `make`. Run fast source checks with
`make lint static-analysis`, or the complete local gate with `make ci`.
`make container-ci` builds the project quality image and runs that gate in its
labeled disposable container. The complete gate also builds the Debian packages
and runs their installed descriptor smoke test through `autopkgtest` in a
disposable source-owned Debian 13 testbed.
`make install` installs the versioned shared object,
public header, and pkg-config metadata. The public contract is documented in
[`include/rptadv_samplerate_adapter/rptadv_samplerate_adapter.h`](include/rptadv_samplerate_adapter/rptadv_samplerate_adapter.h).
`make docs` also checks documented production Rust declarations and publishes
their complete implementation context through Doxygen.
`make test` also checks actual conversion, drift direction, partitioning,
anti-alias rejection, impulse alignment, and repeated reset isolation. Its C
allocation guard interposes glibc allocation calls made inside the shared
FFmpeg libraries, beyond Rust-only allocation tracking.
Project sources and release metadata are hosted at
<https://github.com/cpeter1207/rptadv-samplerate-adapter>.
