/**
 * @file rptadv_samplerate_adapter.h
 * @brief Stable C ABI for rpt_advanced's libsamplerate adapter.
 *
 * This adapter owns libsamplerate's ABI and persistent converter state. Its
 * public operation accepts and produces mono, normalized IEEE-754 binary32
 * PCM in the inclusive nominal full-scale range -1.0 through +1.0.
 */

#ifndef RPTADV_SAMPLERATE_ADAPTER_H
#define RPTADV_SAMPLERATE_ADAPTER_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/** @brief ABI implemented by this adapter descriptor. */
#define RPTADV_SAMPLERATE_ADAPTER_ABI_VERSION 1U

/** @brief Stable capability string exported by the ABI-v1 descriptor. */
#define RPTADV_SAMPLERATE_ADAPTER_CAPABILITY "rptadv.samplerate"

/** @brief Opaque persistent libsamplerate converter state. */
struct rptadv_samplerate_converter;

/** @brief Result returned by an adapter operation. */
enum rptadv_samplerate_adapter_result {
	/** Operation completed. */
	RPTADV_SAMPLERATE_ADAPTER_OK = 0,
	/** A required pointer, converter, channel count, ratio, or frame count was invalid. */
	RPTADV_SAMPLERATE_ADAPTER_INVALID_ARGUMENT = -1,
	/** libsamplerate rejected or failed the requested operation. */
	RPTADV_SAMPLERATE_ADAPTER_LIBSAMPLERATE_ERROR = -2,
	/** The requested adapter capability is not implemented. */
	RPTADV_SAMPLERATE_ADAPTER_UNSUPPORTED = -3,
};

/** @brief libsamplerate sinc quality selections supported by this adapter. */
enum rptadv_samplerate_quality {
	/** Highest-quality band-limited sinc conversion. */
	RPTADV_SAMPLERATE_QUALITY_SINC_BEST = 0,
	/** Medium-quality band-limited sinc conversion. */
	RPTADV_SAMPLERATE_QUALITY_SINC_MEDIUM = 1,
	/** Fastest band-limited sinc conversion. */
	RPTADV_SAMPLERATE_QUALITY_SINC_FASTEST = 2,
};

/**
 * @brief Versioned function table exported by the adapter shared object.
 *
 * A caller obtains this immutable descriptor through
 * @ref rptadv_samplerate_adapter_descriptor and verifies @c abi_version,
 * the capability string, @c struct_size (at least
 * @ref RPTADV_SAMPLERATE_ADAPTER_DESCRIPTOR_V1_MIN_SIZE), and every required
 * function pointer before use. Lifecycle calls are
 * control-plane operations. @ref process is the bounded real-time operation:
 * it does not allocate, lock, block, log, or intentionally panic.
 */
struct rptadv_samplerate_adapter_descriptor {
	/** Size of this descriptor, enabling compatible future extension. */
	uint32_t struct_size;
	/** ABI implemented by every function in this table. */
	uint32_t abi_version;
	/** Stable adapter capability name. */
	const char *capability_name;
	/**
	 * @brief Create one persistent mono converter.
	 *
	 * @param quality One @ref rptadv_samplerate_quality value.
	 * @param channels Required to be one; multichannel conversion is not part of ABI v1.
	 * @param out_converter Destination for the newly owned converter on success.
	 * @return One @ref rptadv_samplerate_adapter_result value.
	 *
	 * The caller must serialize all future operations on the returned handle.
	 */
	enum rptadv_samplerate_adapter_result (*create)(
		enum rptadv_samplerate_quality quality, uint32_t channels,
		struct rptadv_samplerate_converter **out_converter);
	/**
	 * @brief Discard persistent filter history without changing converter quality.
	 *
	 * @param converter Converter obtained from @ref create.
	 * @return One @ref rptadv_samplerate_adapter_result value.
	 */
	enum rptadv_samplerate_adapter_result (*reset)(
		struct rptadv_samplerate_converter *converter);
	/**
	 * @brief Convert one bounded portion of a persistent mono PCM stream.
	 *
	 * @param converter Converter obtained from @ref create.
	 * @param input Input PCM, required when @p input_frames is nonzero.
	 * @param input_frames Available input frames. One frame is one F32 sample.
	 * @param output Output PCM, required when @p output_capacity is nonzero.
	 * @param output_capacity Available output frames. One frame is one F32 sample.
	 * @param ratio Requested output-frame / input-frame ratio, from 1/256 through 256.
	 * @param out_input_used Required destination for the number of consumed input frames.
	 * @param out_output_generated Required destination for the number of generated output frames.
	 * @return One @ref rptadv_samplerate_adapter_result value.
	 *
	 * The operation accepts any bounded input and output block sizes. A
	 * successful call may consume or generate zero frames while the persistent
	 * sinc filter gathers history. Callers resubmit the unconsumed tail on the
	 * next call. Input and output buffers must not overlap. ABI v1 represents a
	 * continuing stream only; it deliberately has no end-of-input tail flush.
	 */
	enum rptadv_samplerate_adapter_result (*process)(
		struct rptadv_samplerate_converter *converter, const float *input,
		uint32_t input_frames, float *output, uint32_t output_capacity,
		double ratio, uint32_t *out_input_used,
		uint32_t *out_output_generated);
	/**
	 * @brief Destroy a converter after all callers have stopped using it.
	 *
	 * @param converter Converter obtained from @ref create, or null.
	 */
	void (*destroy)(struct rptadv_samplerate_converter *converter);
};

/**
 * @brief Minimum readable size of an ABI-v1 descriptor.
 *
 * Consumers must require @c struct_size to be at least this value, rather
 * than requiring equality, so a newer provider can append fields without
 * changing the ABI-v1 prefix.
 */
#define RPTADV_SAMPLERATE_ADAPTER_DESCRIPTOR_V1_MIN_SIZE \
	(offsetof(struct rptadv_samplerate_adapter_descriptor, destroy) + \
	 sizeof(((struct rptadv_samplerate_adapter_descriptor *)0)->destroy))

/*
 * C function parameters are ABI-sensitive. Reject compilation modes such as
 * -fshort-enums that would make these public enum parameters incompatible
 * with Rust's C-int ABI. C11 and C++11 consumers receive the check directly;
 * older language modes retain the documented int-sized ABI requirement.
 */
#if defined(__cplusplus)
static_assert(sizeof(enum rptadv_samplerate_adapter_result) == sizeof(int),
	      "adapter result enum must use the C int ABI");
static_assert(sizeof(enum rptadv_samplerate_quality) == sizeof(int),
	      "adapter quality enum must use the C int ABI");
static_assert(RPTADV_SAMPLERATE_ADAPTER_OK == 0,
	      "adapter result values are part of ABI v1");
static_assert(RPTADV_SAMPLERATE_ADAPTER_INVALID_ARGUMENT == -1,
	      "adapter result values are part of ABI v1");
static_assert(RPTADV_SAMPLERATE_ADAPTER_LIBSAMPLERATE_ERROR == -2,
	      "adapter result values are part of ABI v1");
static_assert(RPTADV_SAMPLERATE_ADAPTER_UNSUPPORTED == -3,
	      "adapter result values are part of ABI v1");
static_assert(RPTADV_SAMPLERATE_QUALITY_SINC_BEST == 0,
	      "adapter quality values are part of ABI v1");
static_assert(RPTADV_SAMPLERATE_QUALITY_SINC_MEDIUM == 1,
	      "adapter quality values are part of ABI v1");
static_assert(RPTADV_SAMPLERATE_QUALITY_SINC_FASTEST == 2,
	      "adapter quality values are part of ABI v1");
#elif defined(__STDC_VERSION__) && __STDC_VERSION__ >= 201112L
_Static_assert(sizeof(enum rptadv_samplerate_adapter_result) == sizeof(int),
	       "adapter result enum must use the C int ABI");
_Static_assert(sizeof(enum rptadv_samplerate_quality) == sizeof(int),
	       "adapter quality enum must use the C int ABI");
_Static_assert(RPTADV_SAMPLERATE_ADAPTER_OK == 0,
	       "adapter result values are part of ABI v1");
_Static_assert(RPTADV_SAMPLERATE_ADAPTER_INVALID_ARGUMENT == -1,
	       "adapter result values are part of ABI v1");
_Static_assert(RPTADV_SAMPLERATE_ADAPTER_LIBSAMPLERATE_ERROR == -2,
	       "adapter result values are part of ABI v1");
_Static_assert(RPTADV_SAMPLERATE_ADAPTER_UNSUPPORTED == -3,
	       "adapter result values are part of ABI v1");
_Static_assert(RPTADV_SAMPLERATE_QUALITY_SINC_BEST == 0,
	       "adapter quality values are part of ABI v1");
_Static_assert(RPTADV_SAMPLERATE_QUALITY_SINC_MEDIUM == 1,
	       "adapter quality values are part of ABI v1");
_Static_assert(RPTADV_SAMPLERATE_QUALITY_SINC_FASTEST == 2,
	       "adapter quality values are part of ABI v1");
#endif

/**
 * @brief Return the immutable ABI-v1 libsamplerate adapter descriptor.
 *
 * @return A process-lifetime descriptor; it must not be freed or modified.
 *
 * Its capability string equals @ref RPTADV_SAMPLERATE_ADAPTER_CAPABILITY.
 */
const struct rptadv_samplerate_adapter_descriptor *
rptadv_samplerate_adapter_descriptor(void);

#ifdef __cplusplus
}
#endif

#endif
