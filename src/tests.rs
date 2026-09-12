//! Deterministic unit tests using a fake libsamplerate function table.

use std::ffi::c_int;
use std::ptr;

use super::{
    AdapterDescriptor, Converter, INVALID_ARGUMENT, LIBSAMPLERATE_ERROR, OK, SINC_BEST,
    SINC_FASTEST, SINC_MEDIUM, UNSUPPORTED, create_with_test_functions, destroy, process, reset,
};
use crate::ffi::{FunctionTable, SrcData, SrcState};

#[repr(C)]
struct FakeState {
    quality: c_int,
    channels: c_int,
    reset_calls: u32,
    process_calls: u32,
    last_ratio: f64,
    reset_error: c_int,
    response: c_int,
}

unsafe extern "C" fn fake_new(quality: c_int, channels: c_int, error: *mut c_int) -> *mut SrcState {
    if error.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        *error = 0;
    }
    Box::into_raw(Box::new(FakeState {
        quality,
        channels,
        reset_calls: 0,
        process_calls: 0,
        last_ratio: 0.0,
        reset_error: 0,
        response: 0,
    }))
    .cast::<SrcState>()
}

unsafe extern "C" fn fake_reset(state: *mut SrcState) -> c_int {
    let Some(state) = (unsafe { state.cast::<FakeState>().as_mut() }) else {
        return 1;
    };
    state.reset_calls += 1;
    state.reset_error
}

unsafe extern "C" fn fake_process(state: *mut SrcState, data: *mut SrcData) -> c_int {
    let (Some(state), Some(data)) = (
        (unsafe { state.cast::<FakeState>().as_mut() }),
        (unsafe { data.as_mut() }),
    ) else {
        return 1;
    };
    state.process_calls += 1;
    state.last_ratio = data.src_ratio;
    match state.response {
        0 => {}
        1 => return 22,
        2 => {
            data.input_frames_used = -1;
            return 0;
        }
        3 => {
            data.input_frames_used = data.input_frames + 1;
            return 0;
        }
        4 => {
            data.output_frames_gen = -1;
            return 0;
        }
        5 => {
            data.output_frames_gen = data.output_frames + 1;
            return 0;
        }
        _ => return 23,
    }
    let frames = data.input_frames.min(data.output_frames);
    if frames > 0 {
        unsafe {
            ptr::copy_nonoverlapping(data.data_in, data.data_out, frames as usize);
        }
    }
    data.input_frames_used = frames;
    data.output_frames_gen = frames;
    0
}

unsafe extern "C" fn fake_delete(state: *mut SrcState) -> *mut SrcState {
    if !state.is_null() {
        unsafe {
            drop(Box::from_raw(state.cast::<FakeState>()));
        }
    }
    ptr::null_mut()
}

unsafe extern "C" fn fake_new_null(
    _quality: c_int,
    _channels: c_int,
    error: *mut c_int,
) -> *mut SrcState {
    if !error.is_null() {
        unsafe {
            *error = 0;
        }
    }
    ptr::null_mut()
}

unsafe extern "C" fn fake_new_with_error(
    quality: c_int,
    channels: c_int,
    error: *mut c_int,
) -> *mut SrcState {
    if error.is_null() {
        return ptr::null_mut();
    }
    let state = unsafe { fake_new(quality, channels, error) };
    unsafe {
        *error = 24;
    }
    state
}

static FAKE_FUNCTIONS: FunctionTable = FunctionTable {
    new: fake_new,
    reset: fake_reset,
    process: fake_process,
    delete: fake_delete,
};

static NULL_NEW_FUNCTIONS: FunctionTable = FunctionTable {
    new: fake_new_null,
    reset: fake_reset,
    process: fake_process,
    delete: fake_delete,
};

static ERROR_NEW_FUNCTIONS: FunctionTable = FunctionTable {
    new: fake_new_with_error,
    reset: fake_reset,
    process: fake_process,
    delete: fake_delete,
};

fn fake_converter(quality: c_int) -> *mut Converter {
    let mut converter = ptr::null_mut();
    assert_eq!(
        create_with_test_functions(&FAKE_FUNCTIONS, quality, 1, &mut converter),
        OK
    );
    assert!(!converter.is_null());
    converter
}

fn fake_state(converter: &Converter) -> &FakeState {
    unsafe { &*converter.state.as_ptr().cast::<FakeState>() }
}

fn fake_state_mut(converter: &mut Converter) -> &mut FakeState {
    unsafe { &mut *converter.state.as_ptr().cast::<FakeState>() }
}

#[test]
fn descriptor_identifies_abi_v1_and_complete_function_table() {
    let descriptor = unsafe { &*super::rptadv_samplerate_adapter_descriptor() };
    assert_eq!(descriptor.abi_version, 1);
    assert!(descriptor.struct_size as usize >= std::mem::size_of::<AdapterDescriptor>());
    assert_eq!(
        unsafe { std::ffi::CStr::from_ptr(descriptor.capability_name) }.to_bytes(),
        &super::CAPABILITY_NAME[..super::CAPABILITY_NAME.len() - 1]
    );
    let _ = descriptor.create;
    let _ = descriptor.reset;
    let _ = descriptor.process;
    let _ = descriptor.destroy;
}

#[test]
fn create_maps_all_supported_sinc_qualities_without_external_types() {
    for quality in [SINC_BEST, SINC_MEDIUM, SINC_FASTEST] {
        let converter = fake_converter(quality);
        let converter_ref = unsafe { &*converter };
        assert_eq!(fake_state(converter_ref).quality, quality);
        assert_eq!(fake_state(converter_ref).channels, 1);
        destroy(converter);
    }
}

#[test]
fn create_rejects_bad_output_quality_and_nonmono_requests() {
    let mut converter = ptr::null_mut();
    assert_eq!(
        create_with_test_functions(&FAKE_FUNCTIONS, SINC_BEST, 1, ptr::null_mut()),
        INVALID_ARGUMENT
    );
    assert_eq!(
        create_with_test_functions(&FAKE_FUNCTIONS, 99, 1, &mut converter),
        INVALID_ARGUMENT
    );
    assert_eq!(
        create_with_test_functions(&FAKE_FUNCTIONS, SINC_BEST, 2, &mut converter),
        UNSUPPORTED
    );
    assert!(converter.is_null());
}

#[test]
fn create_translates_null_and_library_error_states() {
    let mut converter = ptr::null_mut();
    assert_eq!(
        create_with_test_functions(&NULL_NEW_FUNCTIONS, SINC_BEST, 1, &mut converter),
        LIBSAMPLERATE_ERROR
    );
    assert!(converter.is_null());
    assert_eq!(
        create_with_test_functions(&ERROR_NEW_FUNCTIONS, SINC_BEST, 1, &mut converter),
        LIBSAMPLERATE_ERROR
    );
    assert!(converter.is_null());
}

#[test]
fn process_passes_f32_samples_and_arbitrary_block_sizes_directly() {
    let converter = fake_converter(SINC_BEST);
    let input = [-1.0, -0.25, 0.0, 0.75, 1.0];
    let mut output = [9.0; 7];
    let mut input_used = 0;
    let mut output_generated = 0;

    assert_eq!(
        process(
            converter,
            input.as_ptr(),
            input.len() as u32,
            output.as_mut_ptr(),
            output.len() as u32,
            1.5,
            &mut input_used,
            &mut output_generated,
        ),
        OK
    );
    assert_eq!(input_used, input.len() as u32);
    assert_eq!(output_generated, input.len() as u32);
    assert_eq!(&output[..input.len()], &input);
    let converter_ref = unsafe { &*converter };
    assert_eq!(fake_state(converter_ref).process_calls, 1);
    assert_eq!(fake_state(converter_ref).last_ratio, 1.5);
    destroy(converter);
}

#[test]
fn process_accepts_empty_blocks_without_calling_the_external_library() {
    let converter = fake_converter(SINC_BEST);
    let mut input_used = 99;
    let mut output_generated = 99;
    assert_eq!(
        process(
            converter,
            ptr::null(),
            0,
            ptr::null_mut(),
            0,
            1.0,
            &mut input_used,
            &mut output_generated,
        ),
        OK
    );
    assert_eq!((input_used, output_generated), (0, 0));
    assert_eq!(fake_state(unsafe { &*converter }).process_calls, 0);

    let input = [0.0];
    let mut output = [0.0];
    assert_eq!(
        process(
            converter,
            input.as_ptr(),
            1,
            ptr::null_mut(),
            0,
            1.0,
            &mut input_used,
            &mut output_generated,
        ),
        OK
    );
    assert_eq!(
        process(
            converter,
            ptr::null(),
            0,
            output.as_mut_ptr(),
            1,
            1.0,
            &mut input_used,
            &mut output_generated,
        ),
        OK
    );
    destroy(converter);
}

#[test]
fn process_rejects_bad_arguments_and_preserves_zeroed_progress_outputs() {
    let converter = fake_converter(SINC_BEST);
    let input = [0.0];
    let mut output = [0.0];
    let mut input_used = 99;
    let mut output_generated = 99;

    assert_eq!(
        process(
            converter,
            input.as_ptr(),
            1,
            output.as_mut_ptr(),
            1,
            257.0,
            &mut input_used,
            &mut output_generated,
        ),
        INVALID_ARGUMENT
    );
    assert_eq!(
        process(
            converter,
            input.as_ptr(),
            1,
            output.as_mut_ptr(),
            1,
            f64::NAN,
            &mut input_used,
            &mut output_generated,
        ),
        INVALID_ARGUMENT
    );
    assert_eq!((input_used, output_generated), (0, 0));
    assert_eq!(
        process(
            converter,
            ptr::null(),
            1,
            output.as_mut_ptr(),
            1,
            1.0,
            &mut input_used,
            &mut output_generated,
        ),
        INVALID_ARGUMENT
    );
    assert_eq!(
        process(
            converter,
            input.as_ptr(),
            1,
            ptr::null_mut(),
            1,
            1.0,
            &mut input_used,
            &mut output_generated,
        ),
        INVALID_ARGUMENT
    );
    assert_eq!(
        process(
            converter,
            input.as_ptr(),
            1,
            output.as_mut_ptr(),
            1,
            1.0,
            ptr::null_mut(),
            &mut output_generated,
        ),
        INVALID_ARGUMENT
    );
    assert_eq!(
        process(
            converter,
            input.as_ptr(),
            1,
            output.as_mut_ptr(),
            1,
            1.0,
            &mut input_used,
            ptr::null_mut(),
        ),
        INVALID_ARGUMENT
    );
    assert_eq!(
        process(
            ptr::null_mut(),
            input.as_ptr(),
            1,
            output.as_mut_ptr(),
            1,
            1.0,
            &mut input_used,
            &mut output_generated,
        ),
        INVALID_ARGUMENT
    );
    destroy(converter);
}

#[test]
fn reset_and_library_failures_are_translated_without_runtime_fallbacks() {
    let converter = fake_converter(SINC_BEST);
    assert_eq!(reset(converter), OK);
    assert_eq!(fake_state(unsafe { &*converter }).reset_calls, 1);

    let input = [0.25];
    let mut output = [0.0];
    let mut input_used = 99;
    let mut output_generated = 99;
    for response in 1..=5 {
        unsafe {
            fake_state_mut(&mut *converter).response = response;
        }
        assert_eq!(
            process(
                converter,
                input.as_ptr(),
                1,
                output.as_mut_ptr(),
                1,
                1.0,
                &mut input_used,
                &mut output_generated,
            ),
            LIBSAMPLERATE_ERROR
        );
        assert_eq!((input_used, output_generated), (0, 0));
    }
    unsafe {
        fake_state_mut(&mut *converter).reset_error = 25;
    }
    assert_eq!(reset(converter), LIBSAMPLERATE_ERROR);
    assert_eq!(reset(ptr::null_mut()), INVALID_ARGUMENT);
    destroy(converter);
    destroy(ptr::null_mut());
}

#[test]
fn production_descriptor_uses_the_real_dynamic_libsamplerate_backend() {
    let descriptor = unsafe { &*super::rptadv_samplerate_adapter_descriptor() };
    let mut converter = ptr::null_mut();
    let input = [0.0; 512];
    let mut output = [0.0; 1024];
    let mut input_used = 0;
    let mut output_generated = 0;

    assert_eq!((descriptor.create)(SINC_BEST, 1, &mut converter), OK);
    assert!(!converter.is_null());
    assert_eq!(
        (descriptor.process)(
            converter,
            input.as_ptr(),
            input.len() as u32,
            output.as_mut_ptr(),
            output.len() as u32,
            2.0,
            &mut input_used,
            &mut output_generated,
        ),
        OK
    );
    assert!(input_used <= input.len() as u32);
    assert!(output_generated <= output.len() as u32);
    assert_eq!((descriptor.reset)(converter), OK);
    (descriptor.destroy)(converter);
}
