//! Versioned, F32-only libsamplerate adapter for `rpt_advanced`.
//!
//! The public C descriptor contains no libsamplerate types. A converter owns
//! one persistent mono `SRC_STATE`; its `process` entry passes caller F32
//! buffers directly to libsamplerate without a format conversion, allocation,
//! lock, logging operation, or panic path in adapter code.

#![deny(unsafe_op_in_unsafe_fn)]

mod ffi;

use std::ffi::{c_char, c_int, c_long};
use std::mem::size_of;
use std::ptr::{self, NonNull};

/// ABI version exported by this adapter.
const ABI_VERSION: u32 = 1;
/// Stable name used to select this adapter capability.
const CAPABILITY_NAME: &[u8] = b"rptadv.samplerate\0";
/// libsamplerate accepts ratios through this inclusive lower bound.
const MIN_RATIO: f64 = 1.0 / 256.0;
/// libsamplerate accepts ratios through this inclusive upper bound.
const MAX_RATIO: f64 = 256.0;

/// Successful adapter result.
const OK: c_int = 0;
/// Invalid public pointer, ratio, frame count, quality, or channel request.
const INVALID_ARGUMENT: c_int = -1;
/// libsamplerate failed the requested operation.
const LIBSAMPLERATE_ERROR: c_int = -2;
/// ABI v1 intentionally supports mono conversion only.
const UNSUPPORTED: c_int = -3;

/// Highest-quality band-limited sinc conversion.
const SINC_BEST: c_int = 0;
/// Medium-quality band-limited sinc conversion.
const SINC_MEDIUM: c_int = 1;
/// Fastest band-limited sinc conversion.
const SINC_FASTEST: c_int = 2;

const _: () = assert!(SINC_BEST == 0);
const _: () = assert!(SINC_MEDIUM == 1);
const _: () = assert!(SINC_FASTEST == 2);
const _: () = assert!(OK == 0);
const _: () = assert!(INVALID_ARGUMENT == -1);
const _: () = assert!(LIBSAMPLERATE_ERROR == -2);
const _: () = assert!(UNSUPPORTED == -3);

/// Persistent, opaque libsamplerate converter represented by the C ABI.
#[repr(C)]
pub struct Converter {
    state: NonNull<ffi::SrcState>,
    functions: &'static ffi::FunctionTable,
}

impl Drop for Converter {
    /// Release the persistent libsamplerate state owned by this converter.
    fn drop(&mut self) {
        unsafe {
            (self.functions.delete)(self.state.as_ptr());
        }
    }
}

/// Versioned descriptor exposed to C and adapter-neutral Rust consumers.
#[repr(C)]
pub struct AdapterDescriptor {
    struct_size: u32,
    abi_version: u32,
    capability_name: *const c_char,
    create: extern "C" fn(c_int, u32, *mut *mut Converter) -> c_int,
    reset: extern "C" fn(*mut Converter) -> c_int,
    process: extern "C" fn(
        *mut Converter,
        *const f32,
        u32,
        *mut f32,
        u32,
        f64,
        *mut u32,
        *mut u32,
    ) -> c_int,
    destroy: extern "C" fn(*mut Converter),
}

// The descriptor is immutable process-lifetime data. Its raw C string pointer
// and function pointers all refer to immutable static code or storage.
unsafe impl Sync for AdapterDescriptor {}

/// Map one ABI quality selection to its matching libsamplerate converter type.
fn converter_type(quality: c_int) -> Option<c_int> {
    match quality {
        SINC_BEST => Some(SINC_BEST),
        SINC_MEDIUM => Some(SINC_MEDIUM),
        SINC_FASTEST => Some(SINC_FASTEST),
        _ => None,
    }
}

/// Return whether a conversion ratio is finite and supported by libsamplerate.
fn valid_ratio(ratio: f64) -> bool {
    ratio.is_finite() && (MIN_RATIO..=MAX_RATIO).contains(&ratio)
}

/// Create a converter through the supplied production or deterministic test bindings.
fn create_with_functions(
    functions: &'static ffi::FunctionTable,
    quality: c_int,
    channels: u32,
    out_converter: *mut *mut Converter,
) -> c_int {
    let Some(out_converter) = NonNull::new(out_converter) else {
        return INVALID_ARGUMENT;
    };
    unsafe {
        *out_converter.as_ptr() = ptr::null_mut();
    }
    let Some(converter_type) = converter_type(quality) else {
        return INVALID_ARGUMENT;
    };
    if channels != 1 {
        return UNSUPPORTED;
    }

    let mut library_error = 0;
    let state = unsafe { (functions.new)(converter_type, channels as c_int, &mut library_error) };
    let Some(state) = NonNull::new(state) else {
        return LIBSAMPLERATE_ERROR;
    };
    if library_error != 0 {
        unsafe {
            (functions.delete)(state.as_ptr());
        }
        return LIBSAMPLERATE_ERROR;
    }

    let converter = Box::new(Converter { state, functions });
    unsafe {
        *out_converter.as_ptr() = Box::into_raw(converter);
    }
    OK
}

/// One caller-owned F32 conversion request passed through the C ABI boundary.
struct ProcessRequest {
    converter: *mut Converter,
    input: *const f32,
    input_frames: u32,
    output: *mut f32,
    output_capacity: u32,
    ratio: f64,
    out_input_used: *mut u32,
    out_output_generated: *mut u32,
}

/// Validate and execute one bounded conversion through the converter's bindings.
fn process_with_functions(request: ProcessRequest) -> c_int {
    let Some(converter) = NonNull::new(request.converter) else {
        return INVALID_ARGUMENT;
    };
    let (Some(out_input_used), Some(out_output_generated)) = (
        NonNull::new(request.out_input_used),
        NonNull::new(request.out_output_generated),
    ) else {
        return INVALID_ARGUMENT;
    };
    unsafe {
        *out_input_used.as_ptr() = 0;
        *out_output_generated.as_ptr() = 0;
    }
    if !valid_ratio(request.ratio) {
        return INVALID_ARGUMENT;
    }
    if (request.input_frames != 0 && request.input.is_null())
        || (request.output_capacity != 0 && request.output.is_null())
    {
        return INVALID_ARGUMENT;
    }
    if request.input_frames == 0 || request.output_capacity == 0 {
        return OK;
    }

    let converter = unsafe { converter.as_ref() };
    let mut data = ffi::SrcData {
        data_in: request.input,
        data_out: request.output,
        input_frames: request.input_frames as c_long,
        output_frames: request.output_capacity as c_long,
        input_frames_used: 0,
        output_frames_gen: 0,
        end_of_input: 0,
        src_ratio: request.ratio,
    };
    let result = unsafe { (converter.functions.process)(converter.state.as_ptr(), &mut data) };
    if result != 0
        || data.input_frames_used < 0
        || data.input_frames_used > data.input_frames
        || data.output_frames_gen < 0
        || data.output_frames_gen > data.output_frames
    {
        return LIBSAMPLERATE_ERROR;
    }
    unsafe {
        *out_input_used.as_ptr() = data.input_frames_used as u32;
        *out_output_generated.as_ptr() = data.output_frames_gen as u32;
    }
    OK
}

/// Create one converter for the exported production descriptor.
extern "C" fn create(quality: c_int, channels: u32, out_converter: *mut *mut Converter) -> c_int {
    create_with_functions(&ffi::PRODUCTION_FUNCTIONS, quality, channels, out_converter)
}

/// Reset one converter through the exported production descriptor.
extern "C" fn reset(converter: *mut Converter) -> c_int {
    let Some(converter) = NonNull::new(converter) else {
        return INVALID_ARGUMENT;
    };
    let converter = unsafe { converter.as_ref() };
    let result = unsafe { (converter.functions.reset)(converter.state.as_ptr()) };
    if result == 0 { OK } else { LIBSAMPLERATE_ERROR }
}

/// Convert one F32 block through the exported production descriptor.
extern "C" fn process(
    converter: *mut Converter,
    input: *const f32,
    input_frames: u32,
    output: *mut f32,
    output_capacity: u32,
    ratio: f64,
    out_input_used: *mut u32,
    out_output_generated: *mut u32,
) -> c_int {
    process_with_functions(ProcessRequest {
        converter,
        input,
        input_frames,
        output,
        output_capacity,
        ratio,
        out_input_used,
        out_output_generated,
    })
}

/// Destroy one converter through the exported production descriptor.
extern "C" fn destroy(converter: *mut Converter) {
    let Some(converter) = NonNull::new(converter) else {
        return;
    };
    unsafe {
        drop(Box::from_raw(converter.as_ptr()));
    }
}

/// Immutable ABI-v1 production descriptor retained for the process lifetime.
static DESCRIPTOR: AdapterDescriptor = AdapterDescriptor {
    struct_size: size_of::<AdapterDescriptor>() as u32,
    abi_version: ABI_VERSION,
    capability_name: CAPABILITY_NAME.as_ptr().cast::<c_char>(),
    create,
    reset,
    process,
    destroy,
};

/// Return the immutable function table for ABI version one.
#[unsafe(no_mangle)]
pub extern "C" fn rptadv_samplerate_adapter_descriptor() -> *const AdapterDescriptor {
    &DESCRIPTOR
}

#[cfg(test)]
/// Create a converter through deterministic test bindings.
pub(crate) fn create_with_test_functions(
    functions: &'static ffi::FunctionTable,
    quality: c_int,
    channels: u32,
    out_converter: *mut *mut Converter,
) -> c_int {
    create_with_functions(functions, quality, channels, out_converter)
}

#[cfg(test)]
mod tests;
