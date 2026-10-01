/**
 * @file rptadv_samplerate_adapter.h
 * @brief ABI2 for prepared, bounded mono F32 sample-rate conversion.
 *
 * PCM uses normalized IEEE-754 binary32, nominally -1.0 through +1.0.
 * Each handle has one serialized owner. Creation and destruction are control
 * operations; processing, reset, and queue observation retain prepared storage.
 */
#ifndef RPTADV_SAMPLERATE_ADAPTER_H
#define RPTADV_SAMPLERATE_ADAPTER_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/** Descriptor ABI; incompatible ABI1 providers must be rejected. */
#define RPTADV_SAMPLERATE_ADAPTER_ABI_VERSION 2U
/** Stable capability name, independent of the backend implementation. */
#define RPTADV_SAMPLERATE_ADAPTER_CAPABILITY "rptadv.samplerate"

/** Exclusively owned, opaque prepared converter. */
struct rptadv_samplerate_converter;

/** Result codes have the platform C-int representation. */
enum rptadv_samplerate_adapter_result {
	RPTADV_SAMPLERATE_ADAPTER_OK = 0, /**< Operation completed. */
	RPTADV_SAMPLERATE_ADAPTER_INVALID_ARGUMENT = -1, /**< Invalid pointer or value. */
	RPTADV_SAMPLERATE_ADAPTER_BACKEND_ERROR = -2, /**< Backend or allocation failure. */
	RPTADV_SAMPLERATE_ADAPTER_UNSUPPORTED = -3, /**< Unsupported rate or frame bound. */
};

/** Immutable process-lifetime capability descriptor. */
struct rptadv_samplerate_adapter_descriptor {
	uint32_t struct_size; /**< Readable descriptor size. */
	uint32_t abi_version; /**< ABI implemented by all functions. */
	const char *capability_name; /**< Stable capability identifier. */
	/**
	 * Prepare one mono converter with fixed nominal rates and call bounds.
	 * @param input_rate_hz Nominal source rate, positive and at most INT_MAX.
	 * @param output_rate_hz Nominal destination rate, positive and at most INT_MAX.
	 * @param maximum_input_frames Maximum input count per process call.
	 * @param maximum_output_frames Maximum output capacity per process call.
	 * @param out_converter Owned handle on success; null on failure.
	 * @return An adapter result. Nominal ratios range from 1/256 through 256.
	 * Frame bounds must be positive and representable by the backend. Setup
	 * allocates storage and warms the selected SWR Kaiser filter: size 64,
	 * cutoff 1.0. There is no variable quality or multichannel selector.
	 */
	enum rptadv_samplerate_adapter_result (*create)(uint32_t input_rate_hz,
		uint32_t output_rate_hz, uint32_t maximum_input_frames,
		uint32_t maximum_output_frames,
		struct rptadv_samplerate_converter **out_converter);
	/**
	 * Discard an ended burst and correction without allocation or stale audio.
	 * @param converter Owned prepared converter.
	 * @return An adapter result.
	 * The bounded operation feeds and discards prepared silence to replace old
	 * FIR history. It retains fractional phase and intrinsic filter latency.
	 */
	enum rptadv_samplerate_adapter_result (*reset)(struct rptadv_samplerate_converter *converter);
	/**
	 * Convert a bounded portion of a continuing stream without allocation.
	 * @param converter Owned prepared converter.
	 * @param input Disjoint mono F32 input; may be null only for zero frames.
	 * @param input_frames Available input, within the configured maximum.
	 * @param output Disjoint output; may be null only for zero capacity.
	 * @param output_capacity Output capacity, within the configured maximum.
	 * @param ratio Output/input ratio within nominal * [0.999, 1.001].
	 * @param out_input_used Accepted input prefix, including internally queued PCM.
	 * @param out_output_generated Actual samples written, without final-tail padding.
	 * @return An adapter result.
	 * Resubmit the unaccepted input tail. Zero input drains available output;
	 * zero output capacity is a no-op. This interface never flushes EOF.
	 * Startup/reset history is zero-extended; intrinsic FIR latency remains.
	 * Validation failures leave the two output counts zero when their pointers
	 * and converter are valid. A backend failure requires a successful reset.
	 */
	enum rptadv_samplerate_adapter_result (*process)(struct rptadv_samplerate_converter *converter,
		const float *input, uint32_t input_frames, float *output,
		uint32_t output_capacity, double ratio, uint32_t *out_input_used,
		uint32_t *out_output_generated);
	/** Destroy a stopped converter; null is allowed. */
	void (*destroy)(struct rptadv_samplerate_converter *converter);
	/**
	 * Observe queued input beyond intrinsic FIR lookahead, without allocation.
	 * @param converter Owned prepared converter.
	 * @param out_frames Additional input-domain backlog, rounded to whole frames.
	 * @return An adapter result. This excludes the warmed filter delay and can
	 * differ by one input frame with fractional phase; it is not a sample tag.
	 */
	enum rptadv_samplerate_adapter_result (*queued_input)(struct rptadv_samplerate_converter *converter,
		uint32_t *out_frames);
	/**
	 * Obtain intrinsic setup delay, rounded to the nearest output frame.
	 * @param converter Owned prepared converter.
	 * @param out_frames Immutable warmed delay for finite-media prefix trimming.
	 * @return An adapter result. Excludes queue backlog and PLC. Streaming
	 * compensation and burst reset may retain a different fractional phase;
	 * this reports the nominal delay measured at construction.
	 */
	enum rptadv_samplerate_adapter_result (*converter_output_delay)(struct rptadv_samplerate_converter *converter,
		uint32_t *out_frames);
};

/** Minimum readable ABI2 descriptor prefix. */
#define RPTADV_SAMPLERATE_ADAPTER_DESCRIPTOR_V2_MIN_SIZE \
	(offsetof(struct rptadv_samplerate_adapter_descriptor, converter_output_delay) + \
	 sizeof(((struct rptadv_samplerate_adapter_descriptor *)0)->converter_output_delay))

#if defined(__cplusplus)
static_assert(sizeof(enum rptadv_samplerate_adapter_result) == sizeof(int), "C-int ABI required");
#elif defined(__STDC_VERSION__) && __STDC_VERSION__ >= 201112L
_Static_assert(sizeof(enum rptadv_samplerate_adapter_result) == sizeof(int), "C-int ABI required");
#endif

/** Return the immutable ABI2 descriptor. */
const struct rptadv_samplerate_adapter_descriptor *rptadv_samplerate_adapter_descriptor(void);

#ifdef __cplusplus
}
#endif
#endif
