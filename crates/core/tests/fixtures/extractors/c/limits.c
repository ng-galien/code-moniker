#include <assert.h>

// cm: def max len define
#define MAX_LEN 4096
// cm: def clamp macro
#define CLAMP(x, lo, hi) ((x) < (lo) ? (lo) : ((x) > (hi) ? (hi) : (x)))

// cm: def limit mode enum
enum limit_mode {
	// cm: def soft constant
	LIMIT_SOFT,
	LIMIT_HARD = 2,
};

// anonymous enums leak their constants into the module scope
enum {
	FLAG_NONE = 0,
	// cm: def module level flag
	FLAG_STRICT = 1,
};

#ifdef _WIN32
static int platform_limit(void) { return 1; }
#else
// cm: def posix branch function
static int platform_limit(void) { return 2; }
#endif

// cm: def apply limit
int apply_limit(int value) {
	// cm: ref apply calls the clamp macro
	int bounded = CLAMP(value, 0, MAX_LEN);
	// cm: ref apply calls platform branch
	int base = platform_limit();
	// cm: ref bare call stays a name claim
	return merge_limits(bounded, base);
}
