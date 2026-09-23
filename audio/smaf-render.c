// Bridge the LGPL WildMIDI library to the browser's AudioBuffer renderer.
#include "wildmidi_lib.h"
#include <stdint.h>

int smaf_init(void) {
    return WildMidi_Init("@opl3", 44100, 0);
}

uintptr_t smaf_open(const uint8_t *data, uint32_t length) {
    return (uintptr_t)WildMidi_OpenBuffer(data, length);
}

int smaf_read(uintptr_t handle, int8_t *output, uint32_t length) {
    return WildMidi_GetOutput((midi *)handle, output, length);
}

void smaf_close(uintptr_t handle) {
    WildMidi_Close((midi *)handle);
}
