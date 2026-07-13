use crate::core::moniker::{Moniker, MonikerBuilder};

use super::super::kinds;

pub(super) fn is_go_primitive(name: &[u8]) -> bool {
	matches!(
		name,
		b"bool"
			| b"byte" | b"complex64"
			| b"complex128"
			| b"error"
			| b"float32"
			| b"float64"
			| b"int" | b"int8"
			| b"int16"
			| b"int32"
			| b"int64"
			| b"rune" | b"string"
			| b"uint" | b"uint8"
			| b"uint16"
			| b"uint32"
			| b"uint64"
			| b"uintptr"
			| b"any"
	)
}

pub(super) fn is_builtin_func(name: &[u8]) -> bool {
	BUILTIN_FUNCS
		.binary_search_by(|candidate| candidate.as_bytes().cmp(name))
		.is_ok()
}

pub(super) fn builtin_func_target(root: &Moniker, name: &[u8]) -> Moniker {
	let mut builder = MonikerBuilder::new();
	builder.project(root.as_view().project());
	builder.segment(kinds::EXTERNAL_PKG, b"builtin");
	builder.segment(kinds::FUNC, name);
	builder.build()
}

// Universe-block predeclared functions (Go spec, "Predeclared identifiers").
const BUILTIN_FUNCS: &[&str] = &[
	"append", "cap", "clear", "close", "complex", "copy", "delete", "imag", "len", "make", "max",
	"min", "new", "panic", "print", "println", "real", "recover",
];

// Top-level import roots of the Go standard library (`go list std`, deduped
// to the first path piece). The linkage layer keys manifest-free external
// classification on this set.
pub(crate) const STDLIB_PACKAGES: &[&str] = &[
	"archive",
	"bufio",
	"builtin",
	"bytes",
	"cmp",
	"compress",
	"container",
	"context",
	"crypto",
	"database",
	"debug",
	"embed",
	"encoding",
	"errors",
	"expvar",
	"flag",
	"fmt",
	"go",
	"hash",
	"html",
	"image",
	"index",
	"io",
	"iter",
	"log",
	"maps",
	"math",
	"mime",
	"net",
	"os",
	"path",
	"plugin",
	"reflect",
	"regexp",
	"runtime",
	"slices",
	"sort",
	"strconv",
	"strings",
	"structs",
	"sync",
	"syscall",
	"testing",
	"text",
	"time",
	"unicode",
	"unique",
	"unsafe",
	"weak",
];

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn stdlib_packages_sorted_unique() {
		for pair in STDLIB_PACKAGES.windows(2) {
			assert!(
				pair[0] < pair[1],
				"expected sorted unique stdlib list, got `{}` before `{}`",
				pair[0],
				pair[1]
			);
		}
	}

	#[test]
	fn builtin_funcs_sorted_unique() {
		for pair in BUILTIN_FUNCS.windows(2) {
			assert!(
				pair[0] < pair[1],
				"expected sorted unique builtin list, got `{}` before `{}`",
				pair[0],
				pair[1]
			);
		}
	}

	#[test]
	fn builtin_funcs_are_language_contract() {
		for name in [
			b"len".as_slice(),
			b"append",
			b"make",
			b"new",
			b"panic",
			b"recover",
			b"close",
			b"delete",
		] {
			assert!(is_builtin_func(name));
		}
		assert!(!is_builtin_func(b"Len"));
		assert!(!is_builtin_func(b"fmt"));
	}
}
