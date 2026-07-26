use std::collections::HashMap;

// cm: def normalize function
fn normalize(values: Vec<String>) -> Vec<String> {
	// cm: ref normalize instantiates hash map
	let mut seen = HashMap::new();
	let mut out = Vec::new();
	// cm: ref normalize calls iterator map
	for value in values.into_iter().map(|v| v.to_string()) {
		if value.len() > 1 {
			seen.insert(value.clone(), value.len());
			out.push(value);
		}
	}
	vec![format!("{:?}", seen), out.join(",")]
}

// cm: def local unresolved caller
fn local_project_call() {
	// cm: ref missing project function remains unresolved
	missing_project_function();
}

// cm: def parent helper
fn parent_helper() {}

mod nested {
	// cm: def nested caller
	pub fn nested_caller() {
		// cm: ref super call anchors on the parent module
		super::parent_helper();
		// cm: ref crate call anchors on the crate root
		crate::parent_helper();
		// cm: ref self call stays in the nested module
		self::nested_sibling();
	}

	// cm: def nested sibling
	pub fn nested_sibling() {}
}

// cm: def dispatcher
fn dispatcher() {
	// cm: ref sibling module call anchors on the lexical module
	nested::nested_sibling();
}
