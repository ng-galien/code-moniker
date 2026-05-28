pub(super) fn is_primitive_type(name: &[u8]) -> bool {
	JAVA_PRIMITIVE_TYPES
		.binary_search_by(|candidate| candidate.as_bytes().cmp(name))
		.is_ok()
}

pub(super) fn is_java_lang_type(name: &[u8]) -> bool {
	let len = name.len();
	if !(JAVA_LANG_MIN_TYPE_NAME_LEN..=JAVA_LANG_MAX_TYPE_NAME_LEN).contains(&len) {
		return false;
	}
	let Some(first) = name.first() else {
		return false;
	};
	if !first.is_ascii_uppercase() {
		return false;
	}
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

const JAVA_LANG_MIN_TYPE_NAME_LEN: usize = 4;
const JAVA_LANG_MAX_TYPE_NAME_LEN: usize = 31;

// Java compilation units implicitly import public type names declared directly
// in java.lang. Keep this aligned with Java SE 17, the fixture test release.
const JAVA_LANG_TYPES: &[&str] = &[
	"AbstractMethodError",
	"Appendable",
	"ArithmeticException",
	"ArrayIndexOutOfBoundsException",
	"ArrayStoreException",
	"AssertionError",
	"AutoCloseable",
	"Boolean",
	"BootstrapMethodError",
	"Byte",
	"CharSequence",
	"Character",
	"Class",
	"ClassCastException",
	"ClassCircularityError",
	"ClassFormatError",
	"ClassLoader",
	"ClassNotFoundException",
	"ClassValue",
	"CloneNotSupportedException",
	"Cloneable",
	"Comparable",
	"Compiler",
	"Deprecated",
	"Double",
	"Enum",
	"EnumConstantNotPresentException",
	"Error",
	"Exception",
	"ExceptionInInitializerError",
	"Float",
	"FunctionalInterface",
	"IllegalAccessError",
	"IllegalAccessException",
	"IllegalArgumentException",
	"IllegalCallerException",
	"IllegalMonitorStateException",
	"IllegalStateException",
	"IllegalThreadStateException",
	"IncompatibleClassChangeError",
	"IndexOutOfBoundsException",
	"InheritableThreadLocal",
	"InstantiationError",
	"InstantiationException",
	"Integer",
	"InternalError",
	"InterruptedException",
	"Iterable",
	"LayerInstantiationException",
	"LinkageError",
	"Long",
	"Math",
	"Module",
	"ModuleLayer",
	"NegativeArraySizeException",
	"NoClassDefFoundError",
	"NoSuchFieldError",
	"NoSuchFieldException",
	"NoSuchMethodError",
	"NoSuchMethodException",
	"NullPointerException",
	"Number",
	"NumberFormatException",
	"Object",
	"OutOfMemoryError",
	"Override",
	"Package",
	"Process",
	"ProcessBuilder",
	"ProcessHandle",
	"Readable",
	"Record",
	"ReflectiveOperationException",
	"Runnable",
	"Runtime",
	"RuntimeException",
	"RuntimePermission",
	"SafeVarargs",
	"SecurityException",
	"SecurityManager",
	"Short",
	"StackOverflowError",
	"StackTraceElement",
	"StackWalker",
	"StrictMath",
	"String",
	"StringBuffer",
	"StringBuilder",
	"StringIndexOutOfBoundsException",
	"SuppressWarnings",
	"System",
	"Thread",
	"ThreadDeath",
	"ThreadGroup",
	"ThreadLocal",
	"Throwable",
	"TypeNotPresentException",
	"UnknownError",
	"UnsatisfiedLinkError",
	"UnsupportedClassVersionError",
	"UnsupportedOperationException",
	"VerifyError",
	"VirtualMachineError",
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
		assert_sorted_unique(JAVA_LANG_TYPES);
		assert_builtin_lookup_bounds(JAVA_LANG_TYPES);
		for name in [
			b"String".as_slice(),
			b"Object",
			b"System",
			b"Deprecated",
			b"SuppressWarnings",
			b"FunctionalInterface",
			b"SafeVarargs",
			b"Appendable",
			b"Math",
			b"RuntimeException",
		] {
			assert!(is_java_lang_type(name));
		}
		assert!(!is_java_lang_type(b"T"));
		assert!(!is_java_lang_type(b"string"));
	}

	fn assert_sorted_unique(values: &[&str]) {
		for pair in values.windows(2) {
			assert!(
				pair[0] < pair[1],
				"expected sorted unique builtin list, got `{}` before `{}`",
				pair[0],
				pair[1]
			);
		}
	}

	fn assert_builtin_lookup_bounds(values: &[&str]) {
		for value in values {
			let name = value.as_bytes();
			assert!(
				(JAVA_LANG_MIN_TYPE_NAME_LEN..=JAVA_LANG_MAX_TYPE_NAME_LEN).contains(&name.len())
			);
			assert!(name[0].is_ascii_uppercase());
			assert!(is_java_lang_type(name));
		}
	}
}
