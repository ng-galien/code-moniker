use std::collections::HashMap;

fn normalize(values: Vec<String>) -> Vec<String> {
	let mut seen = HashMap::new();
	let mut out = Vec::new();
	for value in values.into_iter().map(|v| v.to_string()) {
		if value.len() > 1 {
			seen.insert(value.clone(), value.len());
			out.push(value);
		}
	}
	vec![format!("{:?}", seen), out.join(",")]
}

fn local_project_call() {
	missing_project_function();
}

