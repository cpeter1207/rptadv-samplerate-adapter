//! Private, minimal bindings for the libsamplerate API owned by this adapter.
//!
//! No libsamplerate type crosses the public descriptor in this crate. Tests
//! replace the function table below with a deterministic fake implementation.

use std::ffi::{c_int, c_long};

/// Opaque libsamplerate state.
#[repr(C)]
pub(crate) struct SrcState {
    _private: [u8; 0],
}

/// libsamplerate's streaming process descriptor.
#[repr(C)]
pub(crate) struct SrcData {
    pub(crate) data_in: *const f32,
    pub(crate) data_out: *mut f32,
    pub(crate) input_frames: c_long,
    pub(crate) output_frames: c_long,
    pub(crate) input_frames_used: c_long,
    pub(crate) output_frames_gen: c_long,
    pub(crate) end_of_input: c_int,
    pub(crate) src_ratio: f64,
}

/// The adapter-owned bindings used by a persistent converter.
#[derive(Clone, Copy)]
pub(crate) struct FunctionTable {
    pub(crate) new: unsafe extern "C" fn(c_int, c_int, *mut c_int) -> *mut SrcState,
    pub(crate) reset: unsafe extern "C" fn(*mut SrcState) -> c_int,
    pub(crate) process: unsafe extern "C" fn(*mut SrcState, *mut SrcData) -> c_int,
    pub(crate) delete: unsafe extern "C" fn(*mut SrcState) -> *mut SrcState,
}

#[link(name = "samplerate", kind = "dylib")]
unsafe extern "C" {
    /// Create one persistent converter through libsamplerate's native ABI.
    #[link_name = "src_new"]
    fn libsamplerate_new(
        converter_type: c_int,
        channels: c_int,
        error: *mut c_int,
    ) -> *mut SrcState;
    /// Reset persistent libsamplerate history without changing its converter type.
    #[link_name = "src_reset"]
    fn libsamplerate_reset(state: *mut SrcState) -> c_int;
    /// Convert one streaming block through libsamplerate's native ABI.
    #[link_name = "src_process"]
    fn libsamplerate_process(state: *mut SrcState, data: *mut SrcData) -> c_int;
    /// Destroy one libsamplerate converter and release its native state.
    #[link_name = "src_delete"]
    fn libsamplerate_delete(state: *mut SrcState) -> *mut SrcState;
}

/// Invoke libsamplerate's production converter constructor.
unsafe extern "C" fn production_new(
    converter_type: c_int,
    channels: c_int,
    error: *mut c_int,
) -> *mut SrcState {
    unsafe { libsamplerate_new(converter_type, channels, error) }
}

/// Invoke libsamplerate's production history reset operation.
unsafe extern "C" fn production_reset(state: *mut SrcState) -> c_int {
    unsafe { libsamplerate_reset(state) }
}

/// Invoke libsamplerate's production streaming conversion operation.
unsafe extern "C" fn production_process(state: *mut SrcState, data: *mut SrcData) -> c_int {
    unsafe { libsamplerate_process(state, data) }
}

/// Invoke libsamplerate's production converter destructor.
unsafe extern "C" fn production_delete(state: *mut SrcState) -> *mut SrcState {
    unsafe { libsamplerate_delete(state) }
}

/// Function table used by the exported production descriptor.
pub(crate) static PRODUCTION_FUNCTIONS: FunctionTable = FunctionTable {
    new: production_new,
    reset: production_reset,
    process: production_process,
    delete: production_delete,
};
