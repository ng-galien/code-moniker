#ifndef MATH_H
#define MATH_H

#define MATH_VERSION 1
#define DOUBLE(value) ((value) * 2)
#define TOKEN_KIND(name) MATH_##name
#define MIXED(token, value) MATH_##token + (value)
#define FIELD_VALUE(field) (record.field)
#define TYPE_MACRO(name, c_name) int
#define VARIADIC(first, ...) (first)
#define DECLARE_LOCAL() int injected_value = 7
#define API_IMPORT

extern API_IMPORT int imported_value;

typedef struct MathRecord {
	int value;
} MathRecord;

typedef struct MathBuffer {
	int len;
} MathBuffer;
typedef MathBuffer *MathBufferPtr;

enum MathMode {
	MATH_MODE_FAST = 1,
};

static inline int twice(int value) {
	return value * 2;
}

#endif
