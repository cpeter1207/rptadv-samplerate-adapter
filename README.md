# rptadv-samplerate-adapter

`rptadv-samplerate-adapter` is the versioned dynamic libsamplerate dependency
adapter for `rpt_advanced`. It implements the narrow descriptor specified by
ADR 0022 so controller and ring code have no direct libsamplerate ABI
dependency.

The adapter creates persistent mono `SRC_LINEAR` converters and exchanges
normalized interleaved F32 PCM (`-1.0` through `+1.0`) without format
conversion. ABI v1 continues accepting its best, medium, and fastest quality
selectors, but all select linear interpolation. It dynamically links
`libsamplerate.so`; it neither bundles nor static-links libsamplerate.

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
Project sources and release metadata are hosted at
<https://github.com/cpeter1207/rptadv-samplerate-adapter>.
