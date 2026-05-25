pub(super) fn is_primitive_type(name: &[u8]) -> bool {
	JAVA_PRIMITIVE_TYPES
		.binary_search_by(|candidate| candidate.as_bytes().cmp(name))
		.is_ok()
}

pub(super) fn is_java_lang_type(name: &[u8]) -> bool {
	JAVA_LANG_TYPES
		.binary_search_by(|candidate| candidate.as_bytes().cmp(name))
		.is_ok()
}

pub(super) fn is_inferred_local_type(name: &[u8]) -> bool {
	name == b"var"
}

const JAVA_PRIMITIVE_TYPES: &[&str] = &[
	"boolean", "byte", "char", "double", "float", "int", "long", "short", "void",
];

const JAVA_LANG_TYPES: &[&str] = &[
	"ArithmeticException",
	"AssertionError",
	"AutoCloseable",
	"Boolean",
	"Byte",
	"CharSequence",
	"Character",
	"Class",
	"ClassCastException",
	"ClassLoader",
	"Cloneable",
	"Comparable",
	"Double",
	"Enum",
	"Error",
	"Exception",
	"Float",
	"IllegalArgumentException",
	"IllegalStateException",
	"IndexOutOfBoundsException",
	"Integer",
	"Iterable",
	"Long",
	"Math",
	"NullPointerException",
	"Number",
	"NumberFormatException",
	"Object",
	"Override",
	"Process",
	"ProcessBuilder",
	"Record",
	"Runnable",
	"Runtime",
	"RuntimeException",
	"Short",
	"String",
	"StringBuffer",
	"StringBuilder",
	"System",
	"Thread",
	"ThreadGroup",
	"ThreadLocal",
	"Throwable",
	"UnsupportedOperationException",
	"Void",
];

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn primitive_types_are_language_contract() {
		for name in [
			b"boolean".as_slice(),
			b"byte",
			b"char",
			b"short",
			b"int",
			b"long",
			b"float",
			b"double",
			b"void",
		] {
			assert!(is_primitive_type(name));
		}
	}

	#[test]
	fn java_lang_types_are_language_contract() {
		for name in [
			b"String".as_slice(),
			b"Object",
			b"System",
			b"Math",
			b"RuntimeException",
		] {
			assert!(is_java_lang_type(name));
		}
	}
}
