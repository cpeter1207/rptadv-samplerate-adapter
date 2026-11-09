/** @file swr_contract.c
 * @brief Real-backend streaming, drift, and reset contract checks.
 */
#define _POSIX_C_SOURCE 200809L
#include <assert.h>
#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

/** Independent ABI declaration catches incompatible descriptor publication. */
struct adapter {
	uint32_t size, version;
	const char *capability;
	int (*create)(uint32_t, uint32_t, uint32_t, uint32_t, void **);
	int (*reset)(void *);
	int (*process)(void *, const float *, uint32_t, float *, uint32_t,
		       double, uint32_t *, uint32_t *);
	void (*destroy)(void *);
	int (*queued_input)(void *, uint32_t *);
	int (*converter_output_delay)(void *, uint32_t *);
	int (*converter_input_delay)(void *, int64_t *);
	int (*create_pacing)(uint32_t, uint32_t, uint32_t, uint32_t,
			     double, double, void **);
};
extern const void *rptadv_samplerate_adapter_descriptor(void);
extern void swr_test_realtime_begin(void) __attribute__((weak));
extern void swr_test_realtime_end(void) __attribute__((weak));
extern void swr_test_realtime_checkpoint(const char *) __attribute__((weak));

static void checkpoint(const char *phase)
{
	if (swr_test_realtime_checkpoint)
		swr_test_realtime_checkpoint(phase);
}

static double seconds(void)
{
	struct timespec value;
	assert(clock_gettime(CLOCK_MONOTONIC, &value) == 0);
	return value.tv_sec + value.tv_nsec * 1e-9;
}

static uint32_t impulse_peak(const struct adapter *api, void *handle, double ratio)
{
	float input[256] = {1.0f}, output[256];
	double maximum = 0;
	uint32_t position = 0, peak = UINT32_MAX;
	for (unsigned block = 0; block < 32; ++block) {
		uint32_t used = 0;
		while (used < 256) {
			uint32_t consumed = 0, generated = 0;
			assert(api->process(handle, input + used, 256 - used, output,
					    256, ratio, &consumed, &generated) == 0);
			assert(consumed || generated);
			used += consumed;
			for (uint32_t i = 0; i < generated; ++i, ++position) {
				if (fabs(output[i]) > maximum) {
					maximum = fabs(output[i]);
					peak = position;
				}
			}
		}
		input[0] = 0;
	}
	return peak;
}

static double tone_rms(const struct adapter *api, double frequency)
{
	void *handle = NULL;
	assert(api->create(48000, 8000, 256, 256, &handle) == 0);
	float input[256], output[256];
	double energy = 0;
	uint32_t count = 0;
	for (unsigned block = 0; block < 200; ++block) {
		for (unsigned i = 0; i < 256; ++i)
			input[i] = (float)(0.5 * sin(6.283185307179586 * frequency *
						   (block * 256 + i) / 48000));
		uint32_t used = 0;
		while (used < 256) {
			uint32_t consumed = 0, generated = 0;
			assert(api->process(handle, input + used, 256 - used, output,
					    256, 1.0 / 6.0, &consumed, &generated) == 0);
			assert(consumed || generated);
			used += consumed;
			if (block > 16) {
				for (uint32_t i = 0; i < generated; ++i)
					energy += output[i] * output[i];
				count += generated;
			}
		}
	}
	api->destroy(handle);
	assert(count > 0);
	return sqrt(energy / count);
}

static double maximum_request(const struct adapter *api, void *handle,
			    uint32_t frames, double ratio)
{
	float input[14400], output[256];
	assert(frames <= 14400);
	for (uint32_t i = 0; i < frames; ++i)
		input[i] = 0.5f;
	uint32_t offset = 0;
	while (offset < frames) {
		uint32_t used = 0, generated = 0;
		assert(api->process(handle, input + offset, frames - offset, output,
				    256, ratio, &used, &generated) == 0);
		assert(used || generated);
		offset += used;
	}
	/* Keep the maximum backlog present while exercising reset. */
	double begin = seconds();
	assert(api->reset(handle) == 0);
	return seconds() - begin;
}

static uint64_t run(const struct adapter *api, void *handle, double ratio,
		    unsigned rounds, float level)
{
	float input[256], output[256];
	uint64_t generated = 0;
	for (unsigned i = 0; i < 256; ++i)
		input[i] = level;
	for (unsigned block = 0; block < rounds; ++block) {
		uint32_t used = 0;
		while (used < 256) {
			uint32_t consumed = 0, produced = 0;
			assert(api->process(handle, input + used, 256 - used,
					    output, 256, ratio, &consumed,
					    &produced) == 0);
			assert(consumed <= 256 - used && produced <= 256);
			assert(consumed != 0 || produced != 0);
			used += consumed;
			generated += produced;
			for (uint32_t i = 0; i < produced; ++i) {
				assert(isfinite(output[i]));
				if (level == 0 && output[i] != 0) {
					if (swr_test_realtime_end)
						swr_test_realtime_end();
					fprintf(stderr, "stale output: ratio=%.9f block=%u sample=%u value=%g\n",
						ratio, block, i, (double)output[i]);
					assert(output[i] == 0);
				}
			}
		}
	}
	for (;;) {
		uint32_t consumed = 0, produced = 0;
		assert(api->process(handle, NULL, 0, output, 256, ratio,
				    &consumed, &produced) == 0);
		assert(consumed == 0);
		generated += produced;
		if (!produced)
			break;
	}
	return generated;
}

/** Exercise the full prepared envelope under the real SWR allocation guard. */
static void pacing_contract(const struct adapter *api)
{
	const uint32_t rates[][3] = {{48000, 48000, 256}, {8000, 48000, 6176},
		{48000, 8000, 14400}, {256, 1, 256}, {1, 256, 256}};
	for (unsigned rate = 0; rate < sizeof(rates) / sizeof(rates[0]); ++rate) {
		void *handle = NULL;
		double nominal = (double)rates[rate][1] / rates[rate][0];
		double minimum = nominal * 0.95, maximum = nominal * 1.05;
		assert(api->create_pacing(rates[rate][0], rates[rate][1], rates[rate][2],
					 256, minimum, maximum, &handle) == 0);
		if (swr_test_realtime_begin)
			swr_test_realtime_begin();
		float input[256], output[256];
		for (unsigned i = 0; i < 256; ++i)
			input[i] = 0.25f;
		for (unsigned change = 0; change < 512; ++change) {
			double ratio = minimum + (maximum - minimum) * (change % 17) / 16;
			uint32_t used = 0, generated = 0;
			assert(api->process(handle, input, 256, output, 256, ratio,
					    &used, &generated) == 0);
			assert(used <= 256 && generated <= 256 && (used || generated));
			checkpoint("live pacing changes without reset");
		}
		assert(api->reset(handle) == 0);
		for (unsigned change = 0; change < 64; ++change) {
			double ratio = change % 2 ? minimum : maximum;
			(void)maximum_request(api, handle, rates[rate][2], ratio);
			checkpoint("pacing maximum request and reset");
			uint64_t generated = run(api, handle, ratio, 40, 0.0f);
			double expected = 10240 * ratio;
			assert(fabs((double)generated - expected) < 3 + expected * 0.000001);
			checkpoint("pacing silent stream after reset");
			int64_t position = 0;
			assert(api->converter_input_delay(handle, &position) == 0);
			assert(position > 0);
			assert(api->reset(handle) == 0);
			checkpoint("pacing endpoint changes and position");
		}
		if (swr_test_realtime_end)
			swr_test_realtime_end();
		api->destroy(handle);
	}
}

/** A large legal call retains its ratio and prepared storage past one million outputs. */
static void large_call_contract(const struct adapter *api)
{
	float *input = malloc(2000000 * sizeof(*input));
	float *output = malloc(2100000 * sizeof(*output));
	assert(input != NULL && output != NULL);
	for (unsigned paced = 0; paced < 2; ++paced) {
		void *handle = NULL;
		double ratio = paced ? 0.95 : 0.999;
		assert((paced ? api->create_pacing(48000, 48000, 2000000, 2100000,
						 0.95, 1.05, &handle) :
				api->create(48000, 48000, 2000000, 2100000, &handle)) == 0);
		for (unsigned i = 0; i < 2000000; ++i)
			input[i] = 0.25f;
		if (swr_test_realtime_begin)
			swr_test_realtime_begin();
		for (unsigned burst = 0; burst < 2; ++burst) {
			uint32_t used = 0, generated = 0;
			assert(api->process(handle, input, 2000000, output, 2100000, ratio,
					    &used, &generated) == 0);
			assert(used == 2000000);
			assert(fabs((double)generated - 2000000 * ratio) < 3);
			for (uint32_t i = 0; i < generated; ++i) {
				assert(isfinite(output[i]));
				assert(burst == 0 || output[i] == 0.0f);
			}
			assert(api->reset(handle) == 0);
			checkpoint("large-call conversion and history reset");
			memset(input, 0, 2000000 * sizeof(*input));
		}
		if (swr_test_realtime_end)
			swr_test_realtime_end();
		api->destroy(handle);
	}
	free(output);
	free(input);
}

int main(void)
{
	const struct adapter *api = rptadv_samplerate_adapter_descriptor();
	assert(api != NULL && api->version == 2);
	assert(api->size >= sizeof(*api));
	assert(strcmp(api->capability, "rptadv.samplerate") == 0);
	pacing_contract(api);
	large_call_contract(api);
	double passband = tone_rms(api, 1000);
	double stopband = tone_rms(api, 6000);
	double rejection_db = 20 * log10(passband / stopband);
	fprintf(stderr, "1 kHz RMS=%.9f; 6 kHz RMS=%.9f; rejection=%.2f dB\n",
		passband, stopband, rejection_db);
	assert(passband > 0.35);
	/* Preserve the released filter's rejection at the 2 kHz alias probe. */
	assert(stopband < 0.0001);
	const uint32_t rates[][3] = {{8000, 48000, 256}, {48000, 8000, 256},
		{48000, 48000, 256}, {8000, 48000, 6176}, {48000, 8000, 6176},
		{48000, 48000, 6176}, {8000, 48000, 14400}, {48000, 8000, 14400},
		{48000, 48000, 14400}};
	for (unsigned rate = 0; rate < sizeof(rates) / sizeof(rates[0]); ++rate) {
		void *handle = NULL;
		double ratio = (double)rates[rate][1] / rates[rate][0];
		assert(api->create(rates[rate][0], rates[rate][1], rates[rate][2], 256,
				   &handle) == 0);
		assert(handle != NULL);
		uint32_t delay = 0;
		assert(api->converter_output_delay(handle, &delay) == 0);
		assert(delay > 0);
		uint32_t peak = impulse_peak(api, handle, ratio);
		printf("%u->%u delay=%u impulse_peak=%u\n", rates[rate][0],
		       rates[rate][1], delay, peak);
		assert(peak == delay);
		assert(api->reset(handle) == 0);
		if (swr_test_realtime_begin)
			swr_test_realtime_begin();
		double backlog_reset = maximum_request(api, handle, rates[rate][2], ratio);
		checkpoint("maximum request and reset");
		double begin = seconds();
		uint64_t nominal = run(api, handle, ratio, 400, 0.25f);
		double elapsed = seconds() - begin;
		checkpoint("nominal stream");
		assert(fabs((double)nominal - 102400 * ratio) < 3);
		assert(api->reset(handle) == 0);
		checkpoint("reset before positive correction");
		uint64_t faster = run(api, handle, ratio * 1.001, 400, 0.0f);
		checkpoint("positive correction");
		assert(faster > nominal);
		assert(api->reset(handle) == 0);
		checkpoint("reset before negative correction");
		uint64_t slower = run(api, handle, ratio * 0.999, 400, 0.0f);
		checkpoint("negative correction");
		assert(slower < nominal);
		double maximum_reset = 0;
		for (unsigned reset = 0; reset < 100; ++reset) {
			(void)run(api, handle, ratio, 1, 1.0f);
			checkpoint("nonzero burst");
			begin = seconds();
			assert(api->reset(handle) == 0);
			double reset_elapsed = seconds() - begin;
			if (reset_elapsed > maximum_reset)
				maximum_reset = reset_elapsed;
			checkpoint("burst reset");
			(void)run(api, handle, ratio, 2, 0.0f);
			checkpoint("zero burst after reset");
		}
		uint32_t queued = UINT32_MAX;
		assert(api->queued_input(handle, &queued) == 0);
		assert(queued <= 2);
		if (swr_test_realtime_end)
			swr_test_realtime_end();
		printf("%u->%u max_in=%u mean256input=%.2fus max_reset=%.2fus backlog_reset=%.2fus\n",
		       rates[rate][0], rates[rate][1], rates[rate][2], elapsed * 1e6 / 400,
		       maximum_reset * 1e6, backlog_reset * 1e6);
		api->destroy(handle);
	}
	puts("SWR contract passed");
	return 0;
}
