static void generated_type(TYPE_MACRO(LocalType, LocalType) generated_value) {
	(void) generated_value;
}

int included_value(void) {
	MathRecord record = { .value = 2 };
	MathBuffer buffer = { .len = 1 };
	MathBufferPtr buffer_ptr = &buffer;
	int field_value = FIELD_VALUE(value);
	return DOUBLE(3) + DOUBLE(1, 2) + VARIADIC(1, 2, 3) + MATH_VERSION + MATH_MODE_FAST + TOKEN_KIND(FAST) + MIXED(FAST, mixed_typo) + ordinary_typo + field_value + record.value + buffer_ptr->len;
}
