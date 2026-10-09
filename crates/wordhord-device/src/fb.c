// A thin layer over FBInk, so Rust only sees ints, sizes and paths.
// FBInk's structs are large and change between versions; none of them
// cross into Rust.

#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "fbink.h"

static FBInkConfig cfg = { 0 };

// Opens and sets up the framebuffer. Returns its fd, or -1.
int wh_open(void)
{
	int fd = fbink_open();
	if (fd < 0) {
		return -1;
	}
	cfg.is_quiet = true;
	if (fbink_init(fd, &cfg) < 0) {
		fbink_close(fd);
		return -1;
	}
	return fd;
}

void wh_close(int fd)
{
	fbink_close(fd);
}

// The screen as it's set now: width, height, bits per pixel, and how the
// touch panel's axes relate to it (swapped, mirrored in x, mirrored in y).
void wh_state(uint32_t out[6])
{
	FBInkState s = { 0 };
	fbink_get_state(&cfg, &s);
	out[0] = s.screen_width;
	out[1] = s.screen_height;
	out[2] = s.bpp;
	out[3] = s.touch_swap_axes;
	out[4] = s.touch_mirror_x;
	out[5] = s.touch_mirror_y;
}

// Draws a w×h block of 8-bit grey at (x, y) and refreshes it. A flash
// clears the ghosting of earlier pages; it's slower.
int wh_draw(int fd, const uint8_t* grey, uint32_t w, uint32_t h, uint32_t x, uint32_t y, int flash)
{
	FBInkConfig c  = cfg;
	c.ignore_alpha = true;
	// 1: a flash, clearing ghosts. 2: fast, in black and white only, for
	// what follows a finger (DU). 3: faster still, black on white only, for
	// ink as it's written (A2). 0: as the driver sees fit.
	c.is_flashing  = flash == 1;
	c.wfm_mode     = flash == 1   ? WFM_GC16
			 : flash == 2 ? WFM_DU
			 : flash == 3 ? WFM_A2
				      : WFM_AUTO;
	return fbink_print_raw_data(fd, grey, (int) w, (int) h, (size_t) w * h, (short int) x, (short int) y, &c);
}

// Saves the screen to a file, to put back on exit.
int wh_save(int fd, const char* path)
{
	FBInkDump dump = { 0 };
	if (fbink_dump(fd, &dump) < 0) {
		return -1;
	}
	FILE* f  = fopen(path, "wb");
	int   rc = -1;
	if (f) {
		if (fwrite(&dump, sizeof(dump), 1, f) == 1 && fwrite(dump.data, dump.size, 1, f) == 1) {
			rc = 0;
		}
		fclose(f);
	}
	fbink_free_dump_data(&dump);
	return rc;
}

// Puts back a screen saved by wh_save, with a full refresh.
int wh_restore(int fd, const char* path)
{
	FBInkDump dump = { 0 };
	FILE*     f    = fopen(path, "rb");
	if (!f) {
		return -1;
	}
	int rc = -1;
	if (fread(&dump, sizeof(dump), 1, f) == 1 && dump.size > 0 && dump.size < (64U << 20U)) {
		dump.data = malloc(dump.size);
		if (dump.data && fread(dump.data, dump.size, 1, f) == 1) {
			FBInkConfig c = cfg;
			c.is_flashing = true;
			rc            = fbink_restore(fd, &c, &dump);
		}
		free(dump.data);
	}
	fclose(f);
	return rc;
}

// Opens the touch screen, pen, power button and sleep cover. Fills `fds`
// and `types` (FBInk's INPUT_* bits) and returns how many, at most `max`.
int wh_inputs(int* fds, uint32_t* types, int max)
{
	size_t            count = 0;
	FBInkInputDevice* devs =
	    fbink_input_scan(INPUT_TOUCHSCREEN | INPUT_TABLET | INPUT_POWER_BUTTON | INPUT_SLEEP_COVER |
				 INPUT_PAGINATION_BUTTONS,
			     0U,
			     NO_RECAP,
			     &count);
	if (!devs) {
		return 0;
	}
	int n = 0;
	for (size_t i = 0; i < count; i++) {
		if (devs[i].matched && devs[i].fd != -1 && n < max) {
			fds[n]   = devs[i].fd;
			types[n] = devs[i].type;
			n++;
		}
	}
	free(devs);
	return n;
}
