//! Real-adapter boundary validation and streaming partition regressions.

use super::*;

#[test]
fn prepared_resampler_uses_16_tap_kaiser_filter_and_full_bandwidth_cutoff() {
    unsafe extern "C" {
        fn av_opt_get_double(
            object: *mut std::ffi::c_void,
            name: *const c_char,
            flags: c_int,
            value: *mut f64,
        ) -> c_int;
        fn av_opt_get_int(
            object: *mut std::ffi::c_void,
            name: *const c_char,
            flags: c_int,
            value: *mut i64,
        ) -> c_int;
    }
    let converter = prepare(48000, 8000, 256, 256).unwrap();
    let mut filter_size = 0;
    assert_eq!(
        unsafe {
            av_opt_get_int(
                converter.state.as_ptr().cast(),
                c"filter_size".as_ptr(),
                0,
                &mut filter_size,
            )
        },
        0
    );
    assert_eq!(filter_size, 16);
    let mut cutoff = 0.0;
    assert_eq!(
        unsafe {
            av_opt_get_double(
                converter.state.as_ptr().cast(),
                c"cutoff".as_ptr(),
                0,
                &mut cutoff,
            )
        },
        0
    );
    assert_eq!(cutoff, 1.0);
}

fn handle() -> *mut Converter {
    let mut value = ptr::null_mut();
    assert_eq!(create(48000, 8000, 256, 256, &mut value), OK);
    assert!(!value.is_null());
    value
}

#[test]
fn constructor_rejects_unrepresentable_setup_and_clears_output() {
    assert_eq!(
        create(48000, 8000, 256, 256, ptr::null_mut()),
        INVALID_ARGUMENT
    );
    for parameters in [
        (0, 8000, 256, 256),
        (48000, 0, 256, 256),
        (48000, 8000, 0, 256),
        (48000, 8000, 256, 0),
    ] {
        let mut value = ptr::dangling_mut();
        assert_eq!(
            create(
                parameters.0,
                parameters.1,
                parameters.2,
                parameters.3,
                &mut value
            ),
            INVALID_ARGUMENT
        );
        assert!(value.is_null());
    }
    for parameters in [
        (u32::MAX, 8000, 256, 256),
        (48000, u32::MAX, 256, 256),
        (48000, 8000, u32::MAX, 256),
        (48000, 8000, 256, u32::MAX),
        (1, 257, 256, 256),
        (257, 1, 256, 256),
        (48000, 8000, c_int::MAX as u32, 256),
    ] {
        let mut value = ptr::dangling_mut();
        assert_eq!(
            create(
                parameters.0,
                parameters.1,
                parameters.2,
                parameters.3,
                &mut value
            ),
            UNSUPPORTED
        );
        assert!(value.is_null());
    }
    assert_eq!(zeroed(usize::MAX), Err(BACKEND_ERROR));
}

#[test]
fn null_lifecycle_and_observation_pointers_fail_safely() {
    let value = handle();
    let mut queued = 42;
    assert_eq!(reset(ptr::null_mut()), INVALID_ARGUMENT);
    assert_eq!(queued_input(ptr::null_mut(), &mut queued), INVALID_ARGUMENT);
    assert_eq!(queued_input(value, ptr::null_mut()), INVALID_ARGUMENT);
    assert_eq!(
        converter_output_delay(ptr::null_mut(), &mut queued),
        INVALID_ARGUMENT
    );
    assert_eq!(
        converter_output_delay(value, ptr::null_mut()),
        INVALID_ARGUMENT
    );
    assert_eq!(converter_output_delay(value, &mut queued), OK);
    assert!(queued > 0);
    assert_eq!(reset(value), OK);
    destroy(value);
    destroy(ptr::null_mut());
}

#[test]
fn invalid_process_requests_return_no_counts_or_buffer_writes() {
    let value = handle();
    let input = [0.5; 257];
    let mut output = [42.0; 257];
    for (input_pointer, input_count, output_null, output_count, ratio) in [
        (ptr::null(), 1, false, 256, 1.0 / 6.0),
        (input.as_ptr(), 1, true, 1, 1.0 / 6.0),
        (input.as_ptr(), 257, false, 256, 1.0 / 6.0),
        (input.as_ptr(), 256, false, 257, 1.0 / 6.0),
        (input.as_ptr(), 256, false, 256, f64::NAN),
        (input.as_ptr(), 256, false, 256, f64::INFINITY),
        (input.as_ptr(), 256, false, 256, 0.998 / 6.0),
        (input.as_ptr(), 256, false, 256, 1.002 / 6.0),
    ] {
        let (mut used, mut generated) = (42, 42);
        let destination = if output_null {
            ptr::null_mut()
        } else {
            output.as_mut_ptr()
        };
        assert_eq!(
            process(
                value,
                input_pointer,
                input_count,
                destination,
                output_count,
                ratio,
                &mut used,
                &mut generated
            ),
            INVALID_ARGUMENT
        );
        assert_eq!((used, generated), (0, 0));
        assert_eq!(output, [42.0; 257]);
    }
    let (mut used, mut generated) = (0, 0);
    assert_eq!(
        process(
            ptr::null_mut(),
            input.as_ptr(),
            1,
            output.as_mut_ptr(),
            1,
            1.0 / 6.0,
            &mut used,
            &mut generated
        ),
        INVALID_ARGUMENT
    );
    assert_eq!(
        process(
            value,
            input.as_ptr(),
            1,
            output.as_mut_ptr(),
            1,
            1.0 / 6.0,
            ptr::null_mut(),
            &mut generated
        ),
        INVALID_ARGUMENT
    );
    assert_eq!(
        process(
            value,
            input.as_ptr(),
            1,
            output.as_mut_ptr(),
            1,
            1.0 / 6.0,
            &mut used,
            ptr::null_mut()
        ),
        INVALID_ARGUMENT
    );
    destroy(value);
}

#[test]
fn zero_output_does_not_accept_input_and_queue_remains_bounded() {
    let value = handle();
    let input = [0.5; 256];
    let mut output = [0.0; 1];
    let (mut used, mut generated, mut queued) = (42, 42, 42);
    assert_eq!(
        process(
            value,
            input.as_ptr(),
            256,
            ptr::null_mut(),
            0,
            1.0 / 6.0,
            &mut used,
            &mut generated
        ),
        OK
    );
    assert_eq!((used, generated), (0, 0));
    for _ in 0..1000 {
        assert_eq!(
            process(
                value,
                input.as_ptr(),
                256,
                output.as_mut_ptr(),
                1,
                1.0 / 6.0,
                &mut used,
                &mut generated
            ),
            OK
        );
        assert!(used <= 256 && generated <= 1);
        assert_eq!(queued_input(value, &mut queued), OK);
        assert!(queued <= 256);
    }
    assert_eq!(reset(value), OK);
    assert_eq!(queued_input(value, &mut queued), OK);
    assert_eq!(queued, 0);
    assert_eq!(
        process(
            value,
            ptr::null(),
            0,
            output.as_mut_ptr(),
            1,
            1.0 / 6.0,
            &mut used,
            &mut generated
        ),
        OK
    );
    assert_eq!((used, generated), (0, 0));
    destroy(value);
}

fn render_partition(input: &[f32], block: usize, capacity: usize) -> Vec<f32> {
    let value = handle();
    let mut result = Vec::new();
    let mut output = [0.0; 256];
    for chunk in input.chunks(block) {
        let mut offset = 0;
        while offset < chunk.len() {
            let (mut used, mut generated) = (0, 0);
            assert_eq!(
                process(
                    value,
                    chunk[offset..].as_ptr(),
                    (chunk.len() - offset) as u32,
                    output.as_mut_ptr(),
                    capacity as u32,
                    1.0 / 6.0,
                    &mut used,
                    &mut generated
                ),
                OK
            );
            assert!(used != 0 || generated != 0);
            offset += used as usize;
            result.extend_from_slice(&output[..generated as usize]);
        }
    }
    loop {
        let (mut used, mut generated) = (0, 0);
        assert_eq!(
            process(
                value,
                ptr::null(),
                0,
                output.as_mut_ptr(),
                capacity as u32,
                1.0 / 6.0,
                &mut used,
                &mut generated
            ),
            OK
        );
        assert_eq!(used, 0);
        result.extend_from_slice(&output[..generated as usize]);
        if generated == 0 {
            break;
        }
    }
    destroy(value);
    result
}

#[test]
fn asymmetric_partitions_produce_identical_pcm_and_counts() {
    let input: Vec<_> = (0..12000)
        .map(|index| (index as f32 * 0.09).sin() * 0.5)
        .collect();
    let expected = render_partition(&input, 256, 256);
    assert_eq!(expected.len(), 2000);
    for (block, output) in [(1, 1), (7, 17), (256, 1), (17, 256)] {
        assert_eq!(render_partition(&input, block, output), expected);
    }
}
