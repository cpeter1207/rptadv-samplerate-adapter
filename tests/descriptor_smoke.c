/**
 * @file descriptor_smoke.c
 * @brief Verify that a C consumer can use the public sample-rate descriptor.
 */

#include <assert.h>
#include <string.h>

#include "rptadv_samplerate_adapter/rptadv_samplerate_adapter.h"

int main(void)
{
	const struct rptadv_samplerate_adapter_descriptor *descriptor;
	struct rptadv_samplerate_converter *converter = NULL;
	float input[512] = { 0.0F };
	float output[1024] = { 0.0F };
	uint32_t input_used = 0;
	uint32_t output_generated = 0;

	descriptor = rptadv_samplerate_adapter_descriptor();
	assert(descriptor != NULL);
	assert(descriptor->abi_version == RPTADV_SAMPLERATE_ADAPTER_ABI_VERSION);
	assert(descriptor->struct_size >=
	       RPTADV_SAMPLERATE_ADAPTER_DESCRIPTOR_V1_MIN_SIZE);
	assert(strcmp(descriptor->capability_name,
		      RPTADV_SAMPLERATE_ADAPTER_CAPABILITY) == 0);
	assert(descriptor->create != NULL);
	assert(descriptor->reset != NULL);
	assert(descriptor->process != NULL);
	assert(descriptor->destroy != NULL);
	assert(descriptor->create(RPTADV_SAMPLERATE_QUALITY_SINC_BEST, 1,
				  &converter) == RPTADV_SAMPLERATE_ADAPTER_OK);
	assert(converter != NULL);
	assert(descriptor->process(converter, input, 512, output, 1024, 2.0,
				   &input_used, &output_generated) ==
	       RPTADV_SAMPLERATE_ADAPTER_OK);
	assert(input_used <= 512);
	assert(output_generated <= 1024);
	assert(descriptor->reset(converter) == RPTADV_SAMPLERATE_ADAPTER_OK);
	descriptor->destroy(converter);
	return 0;
}
