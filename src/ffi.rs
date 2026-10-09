//! Private opaque bindings to the dynamically linked FFmpeg resampler.

use std::ffi::{c_char, c_int, c_void};

/// Opaque FFmpeg resampling context; no native layout crosses our ABI.
#[repr(C)]
pub(crate) struct SwrContext {
    _private: [u8; 0],
}

#[link(name = "swresample", kind = "dylib")]
unsafe extern "C" {
    /// Allocate an unconfigured resampler on the control plane.
    pub(crate) fn swr_alloc() -> *mut SwrContext;
    /// Initialize the configured resampler on the control plane.
    pub(crate) fn swr_init(context: *mut SwrContext) -> c_int;
    /// Release the context and its owned allocations on the control plane.
    pub(crate) fn swr_free(context: *mut *mut SwrContext);
    /// Convert packed mono PCM, accepting all input supplied on success.
    pub(crate) fn swr_convert(
        context: *mut SwrContext,
        output: *const *mut u8,
        output_frames: c_int,
        input: *const *const u8,
        input_frames: c_int,
    ) -> c_int;
    /// Set soft compensation over the specified number of output frames.
    pub(crate) fn swr_set_compensation(
        context: *mut SwrContext,
        sample_delta: c_int,
        distance: c_int,
    ) -> c_int;
    /// Return filter and buffered-input delay in the selected timebase.
    pub(crate) fn swr_get_delay(context: *mut SwrContext, base: i64) -> i64;
}

#[link(name = "avutil", kind = "dylib")]
unsafe extern "C" {
    /// Set one adapter-owned FFmpeg option before initialization.
    pub(crate) fn av_opt_set(
        object: *mut c_void,
        name: *const c_char,
        value: *const c_char,
        flags: c_int,
    ) -> c_int;
    /// Set one integer option before initialization.
    pub(crate) fn av_opt_set_int(
        object: *mut c_void,
        name: *const c_char,
        value: i64,
        flags: c_int,
    ) -> c_int;
    /// Set one floating-point option before initialization.
    pub(crate) fn av_opt_set_double(
        object: *mut c_void,
        name: *const c_char,
        value: f64,
        flags: c_int,
    ) -> c_int;
}
