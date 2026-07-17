#include "math.h"
#include <project/config.h>

int run(void) {
	int (*handler)(void) = 0;
	return DOUBLE(add(1, 2)) + twice(2) + open(1) + hidden(3) + handler() + MATH_VERSION;
}
