#include "math.h"
#include "fragment.c"
#include <assert.h>
#include <project/config.h>
#include <vendor/missing.h>

int run(void) {
	int (*handler)(void) = 0;
	assert(MATH_VERSION > 0);
	return DOUBLE(add(1, 2)) + twice(2) + open(1) + hidden(3) + handler() + MATH_VERSION;
}
