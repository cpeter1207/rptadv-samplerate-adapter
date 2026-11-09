//! Real-adapter boundary validation and streaming partition regressions.

use super::*;

type CreatePacing = extern "C" fn(u32, u32, u32, u32, f64, f64, *mut *mut Converter) -> c_int;

/// Read the appended capability only after checking its independently known prefix.
fn pacing_callback() -> CreatePacing {
    let offset = std::mem::offset_of!(AdapterDescriptor, converter_input_delay)
        + size_of::<extern "C" fn(*mut Converter, *mut i64) -> c_int>();
    let pointer = rptadv_samplerate_adapter_descriptor();
    assert!(
        unsafe { (*pointer).struct_size as usize } >= offset + size_of::<CreatePacing>(),
        "manual-pacing descriptor tail missing"
    );
    unsafe {
        pointer
            .cast::<u8>()
            .add(offset)
            .cast::<CreatePacing>()
            .read()
    }
}

#[test]
fn pacing_is_explicit_and_ordinary_handles_retain_clock_correction_limits() {
    let create_pacing = pacing_callback();
    let input = [0.25; 256];
    let mut output = [0.0; 256];
    for paced in [false, true] {
        let mut converter = ptr::null_mut();
        assert_eq!(
            if paced {
                create_pacing(48000, 48000, 256, 256, 1.0 / 1.02, 1.0, &mut converter)
            } else {
                create(48000, 48000, 256, 256, &mut converter)
            },
            OK
        );
        let (mut used, mut generated) = (42, 42);
        assert_eq!(
            process(
                converter,
                input.as_ptr(),
                256,
                output.as_mut_ptr(),
                256,
                1.0 / 1.02,
                &mut used,
                &mut generated,
            ),
            if paced { OK } else { INVALID_ARGUMENT }
        );
        if paced {
            assert_eq!(used, 256);
            assert!((249..=252).contains(&generated));
        } else {
            assert_eq!((used, generated), (0, 0));
        }
        destroy(converter);
    }
}

#[test]
fn pacing_constructor_validates_prepared_bounds_and_clears_failed_handles() {
    let create_pacing = pacing_callback();
    assert_eq!(
        create_pacing(48000, 48000, 256, 256, 0.95, 1.05, ptr::null_mut()),
        INVALID_ARGUMENT
    );
    for (minimum, maximum) in [
        (f64::NAN, 1.0),
        (1.0, f64::NAN),
        (f64::NEG_INFINITY, 1.0),
        (1.0, f64::INFINITY),
        (1.0, 0.99),
        (0.95, 0.99),
        (1.01, 1.05),
        (0.949999, 1.0),
        (1.0, 1.050001),
    ] {
        let mut converter = ptr::dangling_mut();
        assert_eq!(
            create_pacing(48000, 48000, 256, 256, minimum, maximum, &mut converter),
            INVALID_ARGUMENT
        );
        assert!(converter.is_null());
    }
    let mut converter = ptr::dangling_mut();
    TEST_FAILURE.with(|pending| pending.set(Some(TestFailure::Allocate)));
    assert_eq!(
        create_pacing(48000, 48000, 256, 256, 0.95, 1.05, &mut converter),
        BACKEND_ERROR
    );
    assert!(converter.is_null());
    assert_eq!(
        create_pacing(8000, 48000, 256, 256, 6.0, 6.0, &mut converter),
        OK
    );
    destroy(converter);
    assert_eq!(
        create_pacing(48000, 48000, 256, 256, 1.0, 1.05, &mut converter),
        OK
    );
    destroy(converter);
}

#[test]
fn pacing_warmup_backend_failures_clear_the_output_handle() {
    for (failure, skip) in [(TestFailure::Compensation, 3), (TestFailure::Convert, 6)] {
        TEST_FAILURE_SKIP.with(|remaining| remaining.set(skip));
        TEST_FAILURE.with(|pending| pending.set(Some(failure)));
        let mut converter = ptr::dangling_mut();
        assert_eq!(
            pacing_callback()(48000, 48000, 1, 4096, 0.95, 1.05, &mut converter),
            BACKEND_ERROR
        );
        assert!(converter.is_null());
        assert!(TEST_FAILURE.with(Cell::get).is_none());
    }
}

#[test]
fn pacing_process_rejects_unprepared_ratios_and_invalid_counts_without_writes() {
    let mut converter = ptr::null_mut();
    assert_eq!(
        pacing_callback()(48000, 48000, 256, 256, 1.0 / 1.02, 1.0, &mut converter),
        OK
    );
    let input = [0.25; 257];
    let mut output = [42.0; 257];
    for (count, capacity, ratio) in [
        (256, 256, 0.97),
        (256, 256, 1.001),
        (256, 256, f64::NAN),
        (257, 256, 1.0),
        (256, 257, 1.0),
    ] {
        let (mut used, mut generated) = (42, 42);
        assert_eq!(
            process(
                converter,
                input.as_ptr(),
                count,
                output.as_mut_ptr(),
                capacity,
                ratio,
                &mut used,
                &mut generated,
            ),
            INVALID_ARGUMENT
        );
        assert_eq!((used, generated), (0, 0));
        assert_eq!(output, [42.0; 257]);
    }
    destroy(converter);
}

#[test]
fn pacing_position_tracks_actual_phase_through_rapid_changes_and_reset() {
    let mut converter = ptr::null_mut();
    assert_eq!(
        pacing_callback()(48000, 48000, 512, 256, 0.95, 1.05, &mut converter),
        OK
    );
    let input = [0.25; 512];
    let mut accepted = 0_i64;
    let mut previous = None;
    for index in 0..8192 {
        let mut before = 0;
        assert_eq!(converter_input_delay(converter, &mut before), OK);
        let position = accepted * 1_000_000_000 - before;
        if let Some(previous) = previous {
            let advance = (position - previous) as f64 / 1e9;
            let expected = if index % 2 == 1 {
                1.0 / 0.95
            } else {
                1.0 / 1.05
            };
            assert!((advance - expected).abs() < 0.000001);
        }
        let (mut used, mut generated) = (0, 0);
        let mut output = 0.0;
        assert_eq!(
            process(
                converter,
                input.as_ptr(),
                512,
                &mut output,
                1,
                if index % 2 == 0 { 0.95 } else { 1.05 },
                &mut used,
                &mut generated,
            ),
            OK
        );
        assert_eq!(generated, 1);
        let mut queued = 0;
        assert_eq!(queued_input(converter, &mut queued), OK);
        assert!(queued <= 512);
        accepted += i64::from(used);
        previous = Some(position);
    }
    assert_eq!(reset(converter), OK);
    let mut queued = 42;
    assert_eq!(queued_input(converter, &mut queued), OK);
    assert_eq!(queued, 0);
    for ratio in [0.95, 1.05, 1.0 / 1.02, 1.0] {
        let zeros = [0.0; 512];
        let mut output = [42.0; 256];
        let (mut used, mut generated) = (0, 0);
        assert_eq!(
            process(
                converter,
                zeros.as_ptr(),
                512,
                output.as_mut_ptr(),
                256,
                ratio,
                &mut used,
                &mut generated,
            ),
            OK
        );
        assert!(
            output[..generated as usize]
                .iter()
                .all(|sample| *sample == 0.0)
        );
    }
    destroy(converter);
}

/// Exercise the compatible descriptor tail without requiring the new source name.
fn input_delay_callback() -> extern "C" fn(*mut Converter, *mut i64) -> c_int {
    let pointer = rptadv_samplerate_adapter_descriptor();
    assert!(
        unsafe { (*pointer).struct_size } >= 72,
        "input-delay descriptor tail missing"
    );
    unsafe {
        pointer
            .cast::<u8>()
            .add(64)
            .cast::<extern "C" fn(*mut Converter, *mut i64) -> c_int>()
            .read()
    }
}

#[test]
fn input_delay_tracks_hidden_history_and_live_source_phase() {
    let delay = input_delay_callback();
    for (rate, expected_advance) in [(8000, 166_666_667_i64), (48000, 1_000_000_000)] {
        let mut converter = ptr::null_mut();
        assert_eq!(create(rate, 48000, 512, 256, &mut converter), OK);
        let mut before = 0;
        assert_eq!(delay(converter, &mut before), OK);
        assert!(before > 0); // Includes FIR history even when queued_input is zero.
        let mut queued = 1;
        assert_eq!(queued_input(converter, &mut queued), OK);
        assert_eq!(queued, 0);
        let input = [0.25; 160];
        let mut output = 0.0;
        let (mut used, mut generated) = (0, 0);
        assert_eq!(
            process(
                converter,
                input.as_ptr(),
                160,
                &mut output,
                1,
                48000.0 / f64::from(rate),
                &mut used,
                &mut generated
            ),
            OK
        );
        assert_eq!((used, generated), (160, 1));
        let mut after = 0;
        assert_eq!(delay(converter, &mut after), OK);
        assert!((before + 160_000_000_000 - after - expected_advance).abs() <= 1);
        assert_eq!(delay(ptr::null_mut(), &mut after), INVALID_ARGUMENT);
        assert_eq!(delay(converter, ptr::null_mut()), INVALID_ARGUMENT);
        destroy(converter);
    }
}

#[test]
fn prepared_resampler_uses_size_16_kaiser_filter_and_nyquist_cutoff() {
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
    let converter = prepare(48000, 8000, 256, 256, 0.999 / 6.0, 1.001 / 6.0).unwrap();
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
    let mut filter_type = 0;
    assert_eq!(
        unsafe {
            av_opt_get_int(
                converter.state.as_ptr().cast(),
                c"filter_type".as_ptr(),
                0,
                &mut filter_type,
            )
        },
        0
    );
    assert_eq!(filter_type, 2);
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

#[test]
fn input_delay_tracks_compensation_changes_and_reset_without_losing_phase() {
    for rate in [8000, 44100, 48000] {
        let mut converter = ptr::null_mut();
        assert_eq!(create(rate, 48000, 512, 256, &mut converter), OK);
        let input = [0.0; 256];
        let nominal = 48000.0 / f64::from(rate);
        let mut accepted = 0_i64;
        let mut previous = None;
        for output_index in 0..8192 {
            let mut before = 0;
            assert_eq!(converter_input_delay(converter, &mut before), OK);
            let position = accepted * 1_000_000_000 - before;
            if let Some(previous) = previous {
                let advance = (position - previous) as f64 / 1e9;
                // A one-output step remains correctly located when the next
                // call changes correction; no nominal-ratio extrapolation.
                assert!((advance * nominal - 1.0).abs() < 0.002);
            }
            let correction = if output_index % 1024 < 512 {
                1.001
            } else {
                0.999
            };
            let (mut used, mut generated) = (0, 0);
            let mut output = 0.0;
            assert_eq!(
                process(
                    converter,
                    input.as_ptr(),
                    256,
                    &mut output,
                    1,
                    nominal * correction,
                    &mut used,
                    &mut generated
                ),
                OK
            );
            assert_eq!(generated, 1);
            accepted += i64::from(used);
            previous = Some(position);
        }
        assert_eq!(reset(converter), OK);
        let mut delay = 0;
        assert_eq!(converter_input_delay(converter, &mut delay), OK);
        assert!(delay > 0 && delay < 100_000_000_000);
        destroy(converter);
    }
}

fn handle() -> *mut Converter {
    let mut value = ptr::null_mut();
    assert_eq!(create(48000, 8000, 256, 256, &mut value), OK);
    assert!(!value.is_null());
    value
}

#[test]
fn preparation_reports_each_backend_failure() {
    for failure in [
        TestFailure::Zeroed,
        TestFailure::Allocate,
        TestFailure::Option,
        TestFailure::IntegerOption,
        TestFailure::DoubleOption,
        TestFailure::Initialize,
        TestFailure::Compensation,
    ] {
        TEST_FAILURE.with(|pending| pending.set(Some(failure)));
        assert_eq!(
            prepare(48000, 8000, 256, 256, 0.999 / 6.0, 1.001 / 6.0).map(|_| ()),
            Err(BACKEND_ERROR)
        );
    }
}

#[test]
fn reset_and_processing_report_backend_failures() {
    let value = handle();
    TEST_FAILURE.with(|pending| pending.set(Some(TestFailure::Compensation)));
    assert_eq!(reset(value), BACKEND_ERROR);
    destroy(value);

    let value = handle();
    TEST_FAILURE.with(|pending| pending.set(Some(TestFailure::Convert)));
    assert_eq!(reset(value), BACKEND_ERROR);
    destroy(value);

    let value = handle();
    let input = [0.25; 8];
    let mut output = [0.0; 8];
    let (mut used, mut generated) = (0, 0);
    TEST_FAILURE.with(|pending| pending.set(Some(TestFailure::Compensation)));
    assert_eq!(
        process(
            value,
            input.as_ptr(),
            input.len() as u32,
            output.as_mut_ptr(),
            output.len() as u32,
            1.0 / 6.0,
            &mut used,
            &mut generated,
        ),
        BACKEND_ERROR
    );

    TEST_FAILURE.with(|pending| pending.set(Some(TestFailure::Convert)));
    assert_eq!(
        process(
            value,
            input.as_ptr(),
            input.len() as u32,
            output.as_mut_ptr(),
            output.len() as u32,
            1.0 / 6.0,
            &mut used,
            &mut generated,
        ),
        BACKEND_ERROR
    );
    destroy(value);
}

#[test]
fn reset_discards_ready_output_without_flushing_the_stream() {
    let value = handle();
    let input = [0.25; 256];
    let mut output = [0.0; 1];
    let (mut used, mut generated) = (0, 0);
    assert_eq!(
        process(
            value,
            input.as_ptr(),
            input.len() as u32,
            output.as_mut_ptr(),
            output.len() as u32,
            1.0 / 6.0,
            &mut used,
            &mut generated,
        ),
        OK
    );
    assert_eq!(generated, 1);
    assert_eq!(reset(value), OK);
    destroy(value);
}

#[test]
fn descriptor_is_available_before_converter_creation() {
    assert!(!rptadv_samplerate_adapter_descriptor().is_null());
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

fn render_partition(
    input: &[f32],
    block: usize,
    capacity: usize,
    value: *mut Converter,
    ratio: f64,
) -> (Vec<f32>, i64) {
    let mut initial_delay = 0;
    assert_eq!(converter_input_delay(value, &mut initial_delay), OK);
    let mut result = Vec::new();
    let mut output = vec![0.0; capacity];
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
                    ratio,
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
                ratio,
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
    let mut final_delay = 0;
    assert_eq!(converter_input_delay(value, &mut final_delay), OK);
    let advance = input.len() as i64 * 1_000_000_000 + initial_delay - final_delay;
    destroy(value);
    (result, advance)
}

#[test]
fn asymmetric_partitions_produce_identical_pcm_and_counts() {
    let input: Vec<_> = (0..12000)
        .map(|index| (index as f32 * 0.09).sin() * 0.5)
        .collect();
    let expected = render_partition(&input, 256, 256, handle(), 1.0 / 6.0);
    assert_eq!(expected.0.len(), 2000);
    for (block, output) in [(1, 1), (7, 17), (256, 1), (17, 256)] {
        assert_eq!(
            render_partition(&input, block, output, handle(), 1.0 / 6.0),
            expected
        );
    }
}

#[test]
fn pacing_counts_and_pcm_are_independent_of_input_and_output_partitions() {
    let input: Vec<_> = (0..12000)
        .map(|index| (index as f32 * 0.09).sin() * 0.5)
        .collect();
    for (ratio, count) in [
        (0.95, 11400),
        (1.0 / 1.02, 11765),
        (1.0, 12000),
        (1.05, 12600),
    ] {
        let render = |block, output| {
            let mut converter = ptr::null_mut();
            assert_eq!(
                pacing_callback()(48000, 48000, 256, 256, 0.95, 1.05, &mut converter),
                OK
            );
            render_partition(&input, block, output, converter, ratio)
        };
        let expected = render(256, 256);
        assert!(expected.0.len().abs_diff(count) <= 1);
        for (block, output) in [(1, 1), (7, 17), (256, 1), (17, 256)] {
            assert_eq!(render(block, output), expected);
        }
    }
}

/// Legal calls beyond one million outputs must retain correction for the whole call.
fn check_large_call_partitioning(paced: bool) {
    let input: Vec<_> = (0..2_000_000)
        .map(|index| (index as f32 * 0.09).sin() * 0.5)
        .collect();
    let ratio = if paced { 0.95 } else { 0.999 };
    let render = |block, capacity| {
        let mut converter = ptr::null_mut();
        assert_eq!(
            if paced {
                pacing_callback()(
                    48000,
                    48000,
                    2_000_000,
                    2_100_000,
                    0.95,
                    1.05,
                    &mut converter,
                )
            } else {
                create(48000, 48000, 2_000_000, 2_100_000, &mut converter)
            },
            OK
        );
        render_partition(&input, block, capacity, converter, ratio)
    };
    let expected = render(256, 256);
    let actual = render(2_000_000, 2_100_000);
    assert_eq!(
        actual.0.len(),
        expected.0.len(),
        "large call changed output count"
    );
    assert_eq!(actual.1, expected.1, "large call changed source advance");
    assert!(actual.0 == expected.0, "large call changed PCM");
    let expected_advance = actual.0.len() as f64 / ratio;
    assert!((actual.1 as f64 / 1e9 - expected_advance).abs() < 2.0);
}

#[test]
fn pacing_large_calls_match_partitioned_counts_pcm_and_source_positions() {
    check_large_call_partitioning(true);
}

#[test]
fn ordinary_large_calls_match_partitioned_counts_pcm_and_source_positions() {
    check_large_call_partitioning(false);
}
