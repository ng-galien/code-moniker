#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum Shape {
	Namespace,
	Type,
	Callable,
	Value,
	Annotation,
	Ref,
}

impl Shape {
	pub const ALL: &'static [Shape] = &[
		Shape::Namespace,
		Shape::Type,
		Shape::Callable,
		Shape::Value,
		Shape::Annotation,
		Shape::Ref,
	];

	pub fn as_bytes(self) -> &'static [u8] {
		match self {
			Shape::Namespace => b"namespace",
			Shape::Type => b"type",
			Shape::Callable => b"callable",
			Shape::Value => b"value",
			Shape::Annotation => b"annotation",
			Shape::Ref => b"ref",
		}
	}

	pub fn as_str(self) -> &'static str {
		std::str::from_utf8(self.as_bytes()).unwrap()
	}

	pub fn for_kind(kind: &[u8]) -> Shape {
		shape_of(kind).unwrap_or(Shape::Ref)
	}
}

impl std::str::FromStr for Shape {
	type Err = String;
	fn from_str(s: &str) -> Result<Self, Self::Err> {
		Self::ALL
			.iter()
			.copied()
			.find(|sh| sh.as_str() == s)
			.ok_or_else(|| format!("unknown shape `{s}`"))
	}
}

const SHAPE_TABLE: &[(&[u8], Shape, bool)] = &[
	(b"module", Shape::Namespace, true),
	(b"namespace", Shape::Namespace, true),
	(b"schema", Shape::Namespace, true),
	(b"impl", Shape::Namespace, true),
	(b"class", Shape::Type, true),
	(b"struct", Shape::Type, true),
	(b"interface", Shape::Type, true),
	(b"trait", Shape::Type, true),
	(b"enum", Shape::Type, true),
	(b"record", Shape::Type, true),
	(b"annotation_type", Shape::Type, true),
	(b"table", Shape::Type, true),
	(b"type", Shape::Type, false),
	(b"view", Shape::Type, false),
	(b"delegate", Shape::Type, false),
	(b"function", Shape::Callable, true),
	(b"method", Shape::Callable, true),
	(b"constructor", Shape::Callable, true),
	(b"fn", Shape::Callable, true),
	(b"func", Shape::Callable, true),
	(b"procedure", Shape::Callable, true),
	(b"async_function", Shape::Callable, true),
	(b"field", Shape::Value, false),
	(b"property", Shape::Value, false),
	(b"event", Shape::Value, false),
	(b"enum_constant", Shape::Value, false),
	(b"const", Shape::Value, false),
	(b"static", Shape::Value, false),
	(b"var", Shape::Value, false),
	(b"param", Shape::Value, false),
	(b"local", Shape::Value, false),
	(b"comment", Shape::Annotation, false),
];

pub fn shape_of(kind: &[u8]) -> Option<Shape> {
	SHAPE_TABLE
		.iter()
		.find(|(k, _, _)| *k == kind)
		.map(|(_, s, _)| *s)
}

pub fn opens_scope(kind: &[u8]) -> bool {
	SHAPE_TABLE
		.iter()
		.find(|(k, _, _)| *k == kind)
		.is_some_and(|(_, _, opens)| *opens)
}

pub fn known_kinds() -> impl Iterator<Item = &'static [u8]> {
	SHAPE_TABLE.iter().map(|(k, _, _)| *k)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn shape_table_has_no_duplicate_kind() {
		let mut seen = std::collections::HashSet::new();
		for (k, _, _) in SHAPE_TABLE {
			assert!(seen.insert(*k), "duplicate kind in SHAPE_TABLE: {k:?}");
		}
	}

	#[test]
	fn unknown_kind_has_no_shape() {
		assert!(shape_of(b"definitely_not_a_kind").is_none());
		assert!(!opens_scope(b"definitely_not_a_kind"));
	}

	#[test]
	fn internal_kinds_are_classified() {
		assert_eq!(shape_of(b"module"), Some(Shape::Namespace));
		assert_eq!(shape_of(b"comment"), Some(Shape::Annotation));
		assert_eq!(shape_of(b"local"), Some(Shape::Value));
		assert_eq!(shape_of(b"param"), Some(Shape::Value));
	}

	#[test]
	fn comment_is_the_only_annotation() {
		let annotations: Vec<_> = SHAPE_TABLE
			.iter()
			.filter(|(_, s, _)| *s == Shape::Annotation)
			.map(|(k, _, _)| *k)
			.collect();
		assert_eq!(annotations, vec![b"comment".as_slice()]);
	}

	#[test]
	fn annotation_never_opens_scope() {
		for (_, shape, opens) in SHAPE_TABLE {
			if *shape == Shape::Annotation {
				assert!(!opens, "annotation kind must not open a scope");
			}
		}
	}

	#[test]
	fn values_never_open_scope() {
		for (k, shape, opens) in SHAPE_TABLE {
			if *shape == Shape::Value {
				assert!(!opens, "value kind {k:?} must not open a scope");
			}
		}
	}

	#[test]
	fn callables_always_open_scope() {
		for (k, shape, opens) in SHAPE_TABLE {
			if *shape == Shape::Callable {
				assert!(*opens, "callable kind {k:?} must open a scope");
			}
		}
	}

	#[test]
	fn namespaces_always_open_scope() {
		for (k, shape, opens) in SHAPE_TABLE {
			if *shape == Shape::Namespace {
				assert!(*opens, "namespace kind {k:?} must open a scope");
			}
		}
	}

	#[test]
	fn type_containers_open_scope_aliases_do_not() {
		let containers: &[&[u8]] = &[
			b"class",
			b"struct",
			b"interface",
			b"trait",
			b"enum",
			b"record",
			b"annotation_type",
			b"table",
		];
		let aliases: &[&[u8]] = &[b"type", b"view", b"delegate"];
		for k in containers {
			assert!(opens_scope(k), "type container {k:?} must open a scope");
		}
		for k in aliases {
			assert!(!opens_scope(k), "type alias {k:?} must not open a scope");
		}
	}

	#[test]
	fn shape_str_round_trip_is_lowercase_word() {
		for shape in [
			Shape::Namespace,
			Shape::Type,
			Shape::Callable,
			Shape::Value,
			Shape::Annotation,
		] {
			let s = shape.as_str();
			assert!(s.chars().all(|c| c.is_ascii_lowercase()));
			assert!(!s.is_empty());
		}
	}
}
