use std::borrow::Cow;
use std::cell::RefCell;

use rustc_hash::FxHashMap;
use tree_sitter::Node;

use crate::lang::tree_util::{find_named_child, node_slice};

type ScopeBindings = FxHashMap<Vec<u8>, Option<BindingSite>>;

pub(super) struct ModuleLoads {
	has_create_require: bool,
	bindings: RefCell<FxHashMap<(usize, bool), ScopeBindings>>,
}

impl ModuleLoads {
	pub(super) fn new(source: &str) -> Self {
		Self {
			has_create_require: source.contains("createRequire"),
			bindings: RefCell::default(),
		}
	}
}

#[derive(Clone, Copy)]
struct BindingSite {
	id: usize,
	start: usize,
	end: usize,
}

impl BindingSite {
	fn from_node(node: Node<'_>) -> Self {
		Self {
			id: node.id(),
			start: node.start_byte(),
			end: node.end_byte(),
		}
	}
	fn locate_in(self, scope: Node<'_>) -> Option<Node<'_>> {
		let mut node = scope.descendant_for_byte_range(self.start, self.end)?;
		while node.id() != self.id {
			if node == scope {
				return None;
			}
			node = node.parent()?;
		}
		Some(node)
	}
}

pub(super) fn import_call_specifier<'src>(
	node: Node<'_>,
	source: &'src [u8],
) -> Option<Cow<'src, str>> {
	let callee = node.child_by_field_name("function")?;
	if callee.kind() != "import" {
		return None;
	}
	literal_specifier(first_argument(node)?, source)
}

pub(super) fn require_call_specifier<'src>(
	node: Node<'_>,
	source: &'src [u8],
	loads: &ModuleLoads,
) -> Option<Cow<'src, str>> {
	let callee = node.child_by_field_name("function")?;
	if callee.kind() != "identifier" {
		return None;
	}
	let name = node_slice(callee, source);
	if name != b"require" && !loads.has_create_require {
		return None;
	}
	let specifier = literal_specifier(first_argument(node)?, source)?;
	match visible_binding(callee, name, source, loads) {
		None if name == b"require" => Some(specifier),
		Some(binding) if loads.has_create_require && is_node_loader(binding, source, loads) => {
			Some(specifier)
		}
		_ => None,
	}
}

pub(super) fn literal_specifier<'src>(
	mut node: Node<'_>,
	source: &'src [u8],
) -> Option<Cow<'src, str>> {
	while node.kind() == "parenthesized_expression" {
		node = first_value(node)?;
	}
	match node.kind() {
		"string" => {}
		"template_string" if find_named_child(node, "template_substitution").is_none() => {}
		_ => return None,
	}
	if node.has_error() {
		return None;
	}
	let literal = node.utf8_text(source).ok()?;
	let inner = literal.get(1..literal.len().checked_sub(1)?)?;
	if inner.is_empty() {
		return None;
	}
	if !inner.contains('\\') {
		return Some(Cow::Borrowed(inner));
	}
	decode_escapes(inner)
		.filter(|value| !value.is_empty())
		.map(Cow::Owned)
}

fn decode_escapes(input: &str) -> Option<String> {
	let mut output = String::with_capacity(input.len());
	let mut chars = input.chars().peekable();
	while let Some(ch) = chars.next() {
		if ch != '\\' {
			output.push(ch);
			continue;
		}
		let escaped = match chars.next()? {
			'\n' | '\u{2028}' | '\u{2029}' => continue,
			'\r' => {
				chars.next_if_eq(&'\n');
				continue;
			}
			'n' => '\n',
			'r' => '\r',
			't' => '\t',
			'b' => '\u{8}',
			'f' => '\u{c}',
			'v' => '\u{b}',
			'0' if !chars.peek().is_some_and(char::is_ascii_digit) => '\0',
			'0'..='9' => return None,
			'x' => char::from_u32(read_hex(&mut chars, 2)?)?,
			'u' => unicode_escape(&mut chars)?,
			ch => ch,
		};
		output.push(escaped);
	}
	Some(output)
}

fn read_hex(chars: &mut impl Iterator<Item = char>, length: usize) -> Option<u32> {
	(0..length).try_fold(0, |value, _| Some(value * 16 + chars.next()?.to_digit(16)?))
}

fn unicode_escape(chars: &mut std::iter::Peekable<impl Iterator<Item = char>>) -> Option<char> {
	if chars.next_if_eq(&'{').is_some() {
		let mut count = 0;
		let mut value = 0u32;
		while let Some(digit) = chars.next_if(char::is_ascii_hexdigit) {
			value = value.checked_mul(16)?.checked_add(digit.to_digit(16)?)?;
			count += 1;
		}
		if count > 0 && chars.next_if_eq(&'}').is_some() {
			return char::from_u32(value);
		}
		return None;
	}
	let high = read_hex(chars, 4)?;
	if !(0xD800..=0xDBFF).contains(&high) {
		return char::from_u32(high);
	}
	chars.next_if_eq(&'\\')?;
	chars.next_if_eq(&'u')?;
	let low = read_hex(chars, 4)?;
	if !(0xDC00..=0xDFFF).contains(&low) {
		return None;
	}
	char::from_u32(0x10000 + ((high - 0xD800) << 10) + low - 0xDC00)
}

fn first_argument(node: Node<'_>) -> Option<Node<'_>> {
	first_value(node.child_by_field_name("arguments")?)
}

fn first_value(node: Node<'_>) -> Option<Node<'_>> {
	let mut cursor = node.walk();
	node.named_children(&mut cursor)
		.find(|node| node.kind() != "comment")
}

fn is_node_loader(binding: Node<'_>, source: &[u8], loads: &ModuleLoads) -> bool {
	if binding.kind() != "variable_declarator"
		|| binding
			.parent()
			.and_then(|node| node.child_by_field_name("kind"))
			.is_none_or(|kind| kind.kind() != "const")
		|| binding
			.child_by_field_name("name")
			.is_none_or(|node| node.kind() != "identifier")
	{
		return false;
	}
	let Some(init) = binding
		.child_by_field_name("value")
		.filter(|value| value.kind() == "call_expression")
	else {
		return false;
	};
	init.child_by_field_name("function")
		.is_some_and(|callee| is_node_export(callee, b"createRequire", "module", source, loads))
		&& first_argument(init).is_some_and(|base| current_module_path(base, source, loads))
}

fn current_module_path(node: Node<'_>, source: &[u8], loads: &ModuleLoads) -> bool {
	if is_import_meta_url(node, source) {
		return true;
	}
	if node.kind() == "identifier" && node_slice(node, source) == b"__filename" {
		return visible_binding(node, b"__filename", source, loads).is_none();
	}
	node.kind() == "call_expression"
		&& node
			.child_by_field_name("function")
			.is_some_and(|callee| is_node_export(callee, b"fileURLToPath", "url", source, loads))
		&& first_argument(node).is_some_and(|argument| is_import_meta_url(argument, source))
}

fn is_import_meta_url(node: Node<'_>, source: &[u8]) -> bool {
	node.kind() == "member_expression"
		&& node
			.child_by_field_name("property")
			.is_some_and(|value| node_slice(value, source) == b"url")
		&& node.child_by_field_name("object").is_some_and(|value| {
			value.kind() == "meta_property"
				&& value
					.child(0)
					.is_some_and(|keyword| keyword.kind() == "import")
		})
}

fn is_node_export(
	callee: Node<'_>,
	name: &[u8],
	module: &str,
	source: &[u8],
	loads: &ModuleLoads,
) -> bool {
	let (identifier, binding_kinds) = match callee.kind() {
		"identifier" => (callee, &["import_specifier"][..]),
		"member_expression" => {
			let object = callee.child_by_field_name("object");
			let property = callee.child_by_field_name("property");
			let (Some(object), Some(property)) = (object, property) else {
				return false;
			};
			if object.kind() != "identifier" || node_slice(property, source) != name {
				return false;
			}
			(object, &["namespace_import", "identifier"][..])
		}
		_ => return false,
	};
	let Some(binding) = visible_binding(identifier, node_slice(identifier, source), source, loads)
	else {
		return false;
	};
	if !binding_kinds.contains(&binding.kind()) {
		return false;
	}
	if binding.kind() == "import_specifier"
		&& binding
			.child_by_field_name("name")
			.is_none_or(|imported| node_slice(imported, source) != name)
	{
		return false;
	}
	let mut current = binding;
	while let Some(parent) = current.parent() {
		if parent.kind() == "import_statement" {
			return parent
				.child_by_field_name("source")
				.and_then(|node| literal_specifier(node, source))
				.is_some_and(|value| {
					value == module || value.strip_prefix("node:") == Some(module)
				});
		}
		if !matches!(parent.kind(), "import_clause" | "named_imports") {
			return false;
		}
		current = parent;
	}
	false
}

fn visible_binding<'tree>(
	at: Node<'tree>,
	name: &[u8],
	source: &[u8],
	loads: &ModuleLoads,
) -> Option<Node<'tree>> {
	let mut child = at;
	while let Some(parent) = child.parent() {
		if is_binding_scope(parent)
			&& let Some(binding) = cached_binding(parent, child, name, source, loads)
		{
			return Some(binding);
		}
		child = parent;
	}
	None
}

// Calls in the same lexical scope share a binding lookup. Remember misses too,
// so a series of free require calls does not rescan all module declarations.
fn cached_binding<'tree>(
	scope: Node<'tree>,
	child: Node<'tree>,
	name: &[u8],
	source: &[u8],
	loads: &ModuleLoads,
) -> Option<Node<'tree>> {
	let in_function_body =
		is_function_scope(scope) && scope.child_by_field_name("body") == Some(child);
	let key = (scope.id(), in_function_body);
	let cached = loads
		.bindings
		.borrow()
		.get(&key)
		.and_then(|entries| entries.get(name))
		.copied();
	match cached {
		Some(Some(site)) => site
			.locate_in(scope)
			.or_else(|| binding_in_scope(scope, child, name, source)),
		Some(None) => None,
		None => {
			let binding = binding_in_scope(scope, child, name, source);
			loads
				.bindings
				.borrow_mut()
				.entry(key)
				.or_default()
				.insert(name.to_vec(), binding.map(BindingSite::from_node));
			binding
		}
	}
}

fn is_binding_scope(node: Node<'_>) -> bool {
	matches!(
		node.kind(),
		"program"
			| "statement_block"
			| "class_static_block"
			| "switch_body"
			| "catch_clause"
			| "for_statement"
			| "for_in_statement"
			| "class" | "class_declaration"
	) || is_function_scope(node)
}

fn binding_in_scope<'tree>(
	scope: Node<'tree>,
	child: Node<'tree>,
	name: &[u8],
	source: &[u8],
) -> Option<Node<'tree>> {
	if is_function_scope(scope) {
		return scope
			.child_by_field_name("parameters")
			.or_else(|| scope.child_by_field_name("parameter"))
			.filter(|parameters| pattern_binds(*parameters, name, source))
			.or_else(|| {
				scope.child_by_field_name("name").filter(|identifier| {
					matches!(
						scope.kind(),
						"function_expression"
							| "generator_function"
							| "function_declaration"
							| "generator_function_declaration"
					) && node_slice(*identifier, source) == name
				})
			})
			.or_else(|| {
				scope
					.child_by_field_name("body")
					.filter(|body| *body == child)
					.and_then(|body| hoisted_var_binding(body, name, source))
			});
	}
	match scope.kind() {
		"program" => statement_binding(scope, name, source)
			.or_else(|| hoisted_var_binding(scope, name, source)),
		"statement_block" | "class_static_block" => statement_binding(scope, name, source),
		"switch_body" => {
			let mut cursor = scope.walk();
			scope
				.named_children(&mut cursor)
				.find_map(|branch| statement_binding(branch, name, source))
		}
		"catch_clause" => scope
			.child_by_field_name("parameter")
			.filter(|parameter| pattern_binds(*parameter, name, source)),
		"for_statement" => scope
			.child_by_field_name("initializer")
			.and_then(|initializer| declaration_binding(initializer, name, source)),
		"for_in_statement" if scope.child_by_field_name("kind").is_some() => scope
			.child_by_field_name("left")
			.filter(|pattern| pattern_binds(*pattern, name, source)),
		"class" | "class_declaration" => scope
			.child_by_field_name("name")
			.filter(|identifier| node_slice(*identifier, source) == name),
		_ => None,
	}
}

fn is_function_scope(node: Node<'_>) -> bool {
	matches!(
		node.kind(),
		"function_declaration"
			| "function_expression"
			| "generator_function_declaration"
			| "generator_function"
			| "arrow_function"
			| "method_definition"
	)
}

fn statement_binding<'tree>(scope: Node<'tree>, name: &[u8], source: &[u8]) -> Option<Node<'tree>> {
	let mut cursor = scope.walk();
	scope
		.named_children(&mut cursor)
		.find_map(|statement| declaration_binding(statement, name, source))
}

fn declaration_binding<'tree>(
	decl: Node<'tree>,
	name: &[u8],
	source: &[u8],
) -> Option<Node<'tree>> {
	match decl.kind() {
		"lexical_declaration" | "variable_declaration" => variable_binding(decl, name, source),
		"import_statement" => import_binding(decl, name, source),
		"function_declaration"
		| "generator_function_declaration"
		| "class_declaration"
		| "abstract_class_declaration"
		| "enum_declaration"
		| "internal_module" => decl
			.child_by_field_name("name")
			.filter(|identifier| node_slice(*identifier, source) == name),
		"export_statement" | "ambient_declaration" => statement_binding(decl, name, source),
		_ => None,
	}
}

fn variable_binding<'tree>(decl: Node<'tree>, name: &[u8], source: &[u8]) -> Option<Node<'tree>> {
	let mut cursor = decl.walk();
	decl.named_children(&mut cursor).find(|child| {
		child.kind() == "variable_declarator"
			&& child
				.child_by_field_name("name")
				.is_some_and(|pattern| pattern_binds(pattern, name, source))
	})
}

fn hoisted_var_binding<'tree>(
	node: Node<'tree>,
	name: &[u8],
	source: &[u8],
) -> Option<Node<'tree>> {
	if is_function_scope(node)
		|| matches!(
			node.kind(),
			"class" | "class_declaration" | "class_static_block" | "internal_module"
		) {
		return None;
	}
	if node.kind() == "variable_declaration" {
		return variable_binding(node, name, source);
	}
	if node.kind() == "for_in_statement"
		&& node
			.child_by_field_name("kind")
			.is_some_and(|kind| kind.kind() == "var")
		&& let Some(pattern) = node
			.child_by_field_name("left")
			.filter(|pattern| pattern_binds(*pattern, name, source))
	{
		return Some(pattern);
	}
	let mut cursor = node.walk();
	node.named_children(&mut cursor)
		.find_map(|child| hoisted_var_binding(child, name, source))
}

fn import_binding<'tree>(
	statement: Node<'tree>,
	name: &[u8],
	source: &[u8],
) -> Option<Node<'tree>> {
	let mut cursor = statement.walk();
	if statement
		.children(&mut cursor)
		.any(|child| child.kind() == "type")
	{
		return None;
	}
	if let Some(clause) = find_named_child(statement, "import_require_clause") {
		return find_named_child(clause, "identifier")
			.filter(|child| node_slice(*child, source) == name);
	}
	let clause = find_named_child(statement, "import_clause")?;
	let mut cursor = clause.walk();
	clause
		.named_children(&mut cursor)
		.find_map(|child| match child.kind() {
			"identifier" if node_slice(child, source) == name => Some(child),
			"namespace_import"
				if find_named_child(child, "identifier")
					.is_some_and(|id| node_slice(id, source) == name) =>
			{
				Some(child)
			}
			"named_imports" => {
				let mut cursor = child.walk();
				child.named_children(&mut cursor).find(|specifier| {
					let mut cursor = specifier.walk();
					specifier.kind() == "import_specifier"
						&& !specifier
							.children(&mut cursor)
							.any(|part| part.kind() == "type")
						&& specifier
							.child_by_field_name("alias")
							.or_else(|| specifier.child_by_field_name("name"))
							.is_some_and(|id| node_slice(id, source) == name)
				})
			}
			_ => None,
		})
}

fn pattern_binds(pattern: Node<'_>, name: &[u8], source: &[u8]) -> bool {
	match pattern.kind() {
		"identifier" | "shorthand_property_identifier_pattern" => {
			node_slice(pattern, source) == name
		}
		"assignment_pattern" | "object_assignment_pattern" => pattern
			.child_by_field_name("left")
			.is_some_and(|left| pattern_binds(left, name, source)),
		"pair_pattern" => pattern
			.child_by_field_name("value")
			.is_some_and(|value| pattern_binds(value, name, source)),
		"required_parameter" | "optional_parameter" => pattern
			.child_by_field_name("pattern")
			.or_else(|| pattern.child_by_field_name("name"))
			.is_some_and(|pattern| pattern_binds(pattern, name, source)),
		"formal_parameters" | "array_pattern" | "object_pattern" | "rest_pattern" => {
			let mut cursor = pattern.walk();
			pattern
				.named_children(&mut cursor)
				.any(|child| pattern_binds(child, name, source))
		}
		_ => false,
	}
}
