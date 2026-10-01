//! Versioned mono F32 resampling through a prepared libswresample context.
//!
//! Construction owns allocation and filter preparation. Streaming, queue
//! observation, and burst reset retain the prepared buffers and filter bank.

#![deny(unsafe_op_in_unsafe_fn)]

mod ffi;

use std::ffi::{CStr, c_char, c_int};
use std::mem::size_of;
use std::ptr::{self, NonNull};

/// Descriptor ABI for explicit nominal rates and bounded setup.
const ABI_VERSION: u32 = 2;
/// Successful operation.
const OK: c_int = 0;
/// Invalid pointer, count, rate, or correction.
const INVALID_ARGUMENT: c_int = -1;
/// Backend or control-plane allocation failure.
const BACKEND_ERROR: c_int = -2;
/// Rates or frame counts exceed the backend integer representation.
const UNSUPPORTED: c_int = -3;
/// Compensation duration in output frames; one unit resolves one ppm.
const COMPENSATION_DISTANCE: c_int = 1_000_000;
/// Fixed SWR Kaiser filter size used by each prepared converter.
const FILTER_SIZE: usize = 32;

/// Exclusively owned, fully prepared mono resampler.
pub struct Converter {
    state: NonNull<ffi::SwrContext>,
    input_rate: u32,
    nominal_ratio: f64,
    maximum_input: u32,
    maximum_output: u32,
    zeros: Vec<f32>,
    discard: Vec<f32>,
    baseline_delay: i64,
    output_delay: u32,
}

impl Drop for Converter {
    /// Release native resources after the serialized caller has stopped.
    fn drop(&mut self) {
        let mut state = self.state.as_ptr();
        unsafe { ffi::swr_free(&mut state) };
    }
}

impl Converter {
    /// Convert into prepared discard storage without flushing the stream.
    fn discard_conversion(&mut self, input_frames: usize) -> Result<c_int, c_int> {
        let input = self.zeros.as_ptr().cast::<u8>();
        let output = self.discard.as_mut_ptr().cast::<u8>();
        let result = unsafe {
            ffi::swr_convert(
                self.state.as_ptr(),
                &output,
                self.maximum_output as c_int,
                &input,
                input_frames as c_int,
            )
        };
        if result < 0 {
            Err(BACKEND_ERROR)
        } else {
            Ok(result)
        }
    }

    /// Drain only ready output, retaining the continuing-stream FIR lookahead.
    fn drain_discard(&mut self) -> Result<(), c_int> {
        while self.discard_conversion(0)? != 0 {}
        Ok(())
    }

    /// Replace old PCM with silence without freeing state or changing phase.
    fn clear_history(&mut self) -> Result<(), c_int> {
        if unsafe { ffi::swr_set_compensation(self.state.as_ptr(), 0, 0) } < 0 {
            return Err(BACKEND_ERROR);
        }
        self.drain_discard()?;
        // Scrub the full prepared reserve: FIR-only zeroing can expose stale
        // samples much later when compensation reaches another buffer region.
        self.discard_conversion(self.zeros.len())?;
        self.drain_discard()?;
        self.baseline_delay =
            unsafe { ffi::swr_get_delay(self.state.as_ptr(), i64::from(self.input_rate)) };
        Ok(())
    }

    /// Additional input backlog beyond the warmed, empty FIR lookahead.
    fn queued(&self) -> u32 {
        let delay = unsafe { ffi::swr_get_delay(self.state.as_ptr(), i64::from(self.input_rate)) };
        delay.saturating_sub(self.baseline_delay).max(0) as u32
    }
}

/// Allocate zeroed control-plane storage with recoverable allocation failure.
fn zeroed(length: usize) -> Result<Vec<f32>, c_int> {
    let mut samples = Vec::new();
    samples
        .try_reserve_exact(length)
        .map_err(|_| BACKEND_ERROR)?;
    samples.resize(length, 0.0);
    Ok(samples)
}

/// Apply one string-valued option while the context is uninitialized.
fn option(state: NonNull<ffi::SwrContext>, name: &CStr, value: &CStr) -> Result<(), c_int> {
    if unsafe { ffi::av_opt_set(state.as_ptr().cast(), name.as_ptr(), value.as_ptr(), 0) } < 0 {
        Err(BACKEND_ERROR)
    } else {
        Ok(())
    }
}

/// Apply one integer configuration option before initialization.
fn integer_option(state: NonNull<ffi::SwrContext>, name: &CStr, value: i64) -> Result<(), c_int> {
    if unsafe { ffi::av_opt_set_int(state.as_ptr().cast(), name.as_ptr(), value, 0) } < 0 {
        Err(BACKEND_ERROR)
    } else {
        Ok(())
    }
}

/// Build and warm the complete bounded converter before audio starts.
fn prepare(
    input_rate: u32,
    output_rate: u32,
    maximum_input: u32,
    maximum_output: u32,
) -> Result<Converter, c_int> {
    if [input_rate, output_rate, maximum_input, maximum_output].contains(&0) {
        return Err(INVALID_ARGUMENT);
    }
    if [input_rate, output_rate, maximum_input, maximum_output]
        .iter()
        .any(|value| *value > c_int::MAX as u32)
    {
        return Err(UNSUPPORTED);
    }
    let nominal_ratio = f64::from(output_rate) / f64::from(input_rate);
    if !(1.0 / 256.0..=256.0).contains(&nominal_ratio) {
        return Err(UNSUPPORTED);
    }
    // FFmpeg expands a downsampling FIR by its inverse cutoff. This
    // conservative bound covers its even-length rounding and initial mirror.
    let history = (FILTER_SIZE as f64 / (nominal_ratio * 0.985).min(1.0)).ceil() as usize + 4;
    let warm_length = history
        .checked_mul(4)
        .and_then(|length| length.checked_add(maximum_input as usize * 2))
        .filter(|length| *length <= c_int::MAX as usize)
        .ok_or(UNSUPPORTED)?;
    let zeros = zeroed(warm_length)?;
    let discard = zeroed(maximum_output as usize)?;
    let state = NonNull::new(unsafe { ffi::swr_alloc() }).ok_or(BACKEND_ERROR)?;
    let mut converter = Converter {
        state,
        input_rate,
        nominal_ratio,
        maximum_input,
        maximum_output,
        zeros,
        discard,
        baseline_delay: 0,
        output_delay: 0,
    };
    for (name, value) in [
        (c"in_chlayout", c"mono"),
        (c"out_chlayout", c"mono"),
        (c"in_sample_fmt", c"flt"),
        (c"out_sample_fmt", c"flt"),
        (c"internal_sample_fmt", c"fltp"),
        (c"resampler", c"swr"),
    ] {
        option(state, name, value)?;
    }
    for (name, value) in [
        (c"in_sample_rate", i64::from(input_rate)),
        (c"out_sample_rate", i64::from(output_rate)),
        (c"filter_size", FILTER_SIZE as i64),
        (c"filter_type", 2), // SWR_FILTER_TYPE_KAISER from FFmpeg's public enum.
        (c"flags", 1),       // SWR_FLAG_RESAMPLE also enables equal-rate compensation.
        (c"exact_rational", 0),
    ] {
        integer_option(state, name, value)?;
    }
    if unsafe { ffi::av_opt_set_double(state.as_ptr().cast(), c"cutoff".as_ptr(), 1.0, 0) } < 0 {
        return Err(BACKEND_ERROR);
    }
    if unsafe { ffi::swr_init(state.as_ptr()) } < 0
        || unsafe { ffi::swr_set_compensation(state.as_ptr(), 1, COMPENSATION_DISTANCE) } < 0
    {
        return Err(BACKEND_ERROR);
    }
    // The first warmup can use SWR's initial direct-input path. Repeat with
    // established history to reserve the buffered path used by burst reset.
    for _ in 0..2 {
        converter.clear_history()?;
    }
    converter.output_delay =
        unsafe { ffi::swr_get_delay(state.as_ptr(), i64::from(output_rate)) } as u32;
    Ok(converter)
}

/// Allocate a prepared mono converter, leaving a null result on failure.
extern "C" fn create(
    input_rate: u32,
    output_rate: u32,
    maximum_input: u32,
    maximum_output: u32,
    output: *mut *mut Converter,
) -> c_int {
    let Some(output) = NonNull::new(output) else {
        return INVALID_ARGUMENT;
    };
    unsafe { *output.as_ptr() = ptr::null_mut() };
    match prepare(input_rate, output_rate, maximum_input, maximum_output) {
        Ok(converter) => {
            unsafe { *output.as_ptr() = Box::into_raw(Box::new(converter)) };
            OK
        }
        Err(error) => error,
    }
}

/// Clear the previous burst while retaining every prepared allocation.
extern "C" fn reset(converter: *mut Converter) -> c_int {
    let Some(mut converter) = NonNull::new(converter) else {
        return INVALID_ARGUMENT;
    };
    unsafe { converter.as_mut() }
        .clear_history()
        .map_or_else(|error| error, |()| OK)
}

/// Convert bounded PCM with at most 1000 ppm correction about nominal rates.
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
    let (Some(mut converter), Some(input_used), Some(output_generated)) = (
        NonNull::new(converter),
        NonNull::new(out_input_used),
        NonNull::new(out_output_generated),
    ) else {
        return INVALID_ARGUMENT;
    };
    unsafe {
        *input_used.as_ptr() = 0;
        *output_generated.as_ptr() = 0;
    }
    let converter = unsafe { converter.as_mut() };
    if !ratio.is_finite()
        || ratio < converter.nominal_ratio * 0.999
        || ratio > converter.nominal_ratio * 1.001
        || input_frames > converter.maximum_input
        || output_capacity > converter.maximum_output
        || (input_frames != 0 && input.is_null())
        || (output_capacity != 0 && output.is_null())
    {
        return INVALID_ARGUMENT;
    }
    if output_capacity == 0 {
        return OK;
    }
    let accepted = input_frames.min(converter.maximum_input.saturating_sub(converter.queued()));
    let delta = (f64::from(COMPENSATION_DISTANCE) * (1.0 - converter.nominal_ratio / ratio)).round()
        as c_int;
    if unsafe { ffi::swr_set_compensation(converter.state.as_ptr(), delta, COMPENSATION_DISTANCE) }
        < 0
    {
        return BACKEND_ERROR;
    }
    // Nonnull planes with zero count drain ready PCM without an EOF flush.
    let input_plane = if accepted == 0 {
        converter.zeros.as_ptr()
    } else {
        input
    }
    .cast::<u8>();
    let output_plane = output.cast::<u8>();
    let generated = unsafe {
        ffi::swr_convert(
            converter.state.as_ptr(),
            &output_plane,
            output_capacity as c_int,
            &input_plane,
            accepted as c_int,
        )
    };
    if generated < 0 {
        return BACKEND_ERROR;
    }
    unsafe {
        *input_used.as_ptr() = accepted;
        *output_generated.as_ptr() = generated as u32;
    }
    OK
}

/// Observe additional input backlog without counting intrinsic FIR lookahead.
extern "C" fn queued_input(converter: *mut Converter, output: *mut u32) -> c_int {
    let (Some(converter), Some(output)) = (NonNull::new(converter), NonNull::new(output)) else {
        return INVALID_ARGUMENT;
    };
    unsafe { *output.as_ptr() = converter.as_ref().queued() };
    OK
}

/// Intrinsic setup latency, rounded to the nearest output frame.
extern "C" fn converter_output_delay(converter: *mut Converter, output: *mut u32) -> c_int {
    let (Some(converter), Some(output)) = (NonNull::new(converter), NonNull::new(output)) else {
        return INVALID_ARGUMENT;
    };
    unsafe { *output.as_ptr() = converter.as_ref().output_delay };
    OK
}

/// Destroy one stopped converter; a null handle is a no-op.
extern "C" fn destroy(converter: *mut Converter) {
    if let Some(converter) = NonNull::new(converter) {
        unsafe { drop(Box::from_raw(converter.as_ptr())) };
    }
}

/// Stable C-compatible function table for the prepared converter capability.
#[repr(C)]
pub struct AdapterDescriptor {
    struct_size: u32,
    abi_version: u32,
    capability_name: *const c_char,
    create: extern "C" fn(u32, u32, u32, u32, *mut *mut Converter) -> c_int,
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
    queued_input: extern "C" fn(*mut Converter, *mut u32) -> c_int,
    converter_output_delay: extern "C" fn(*mut Converter, *mut u32) -> c_int,
}

// SAFETY: descriptor code and its capability string are immutable statics.
unsafe impl Sync for AdapterDescriptor {}

/// Immutable process-lifetime ABI2 function table.
static DESCRIPTOR: AdapterDescriptor = AdapterDescriptor {
    struct_size: size_of::<AdapterDescriptor>() as u32,
    abi_version: ABI_VERSION,
    capability_name: c"rptadv.samplerate".as_ptr(),
    create,
    reset,
    process,
    destroy,
    queued_input,
    converter_output_delay,
};

/// Obtain the adapter descriptor before creating any real-time owners.
#[unsafe(no_mangle)]
pub extern "C" fn rptadv_samplerate_adapter_descriptor() -> *const AdapterDescriptor {
    &DESCRIPTOR
}

#[cfg(test)]
mod tests;
