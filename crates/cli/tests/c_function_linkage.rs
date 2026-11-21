use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::Value;

fn query(root: &Path, expression: &str) -> Value {
	let output = Command::new(env!("CARGO_BIN_EXE_code-moniker"))
		.current_dir(root)
		.args(["query", "--root", ".", expression, "--json"])
		.output()
		.expect("run public Code Moniker query");
	assert!(
		output.status.success(),
		"{expression}: {}",
		String::from_utf8_lossy(&output.stderr)
	);
	let response: Value = serde_json::from_slice(&output.stdout).expect("read query JSON");
	response["result"]["data"].clone()
}

fn write(root: &Path, path: &str, text: &str) {
	let file = root.join(path);
	fs::create_dir_all(file.parent().unwrap()).unwrap();
	fs::write(file, text).unwrap();
}

fn callable_uri(root: &Path, name: &str, path: &str) -> Value {
	let symbols = query(
		root,
		&format!("symbol.search name:{name} path:{path} lang:c limit:20"),
	);
	symbols["rows"]
		.as_array()
		.unwrap()
		.iter()
		.find(|row| {
			row["file"] == path
				&& row["name"]
					.as_str()
					.unwrap()
					.starts_with(&format!("{name}("))
		})
		.unwrap_or_else(|| panic!("missing {name} in {path}: {symbols}"))["uri"]
		.clone()
}

fn usage_locations(root: &Path, uri: &Value) -> Vec<String> {
	query(root, &format!("symbol.usages uri:{uri} limit:50"))["rows"]
		.as_array()
		.unwrap()
		.iter()
		.map(|row| row["location"].as_str().unwrap().to_owned())
		.collect()
}

fn common_header_fixture(root: &Path) {
	write(
		root,
		"include/demo/dispatch.h",
		r#"#ifndef DEMO_DISPATCH_H
#define DEMO_DISPATCH_H
#include <stdbool.h>
struct item;
int enqueue(struct item *item);
void action(bool enabled);
int incomplete(int value);
int incompatible(int value);
static inline int schedule(struct item *item) { return enqueue(item); }
#endif
"#,
	);
	write(
		root,
		"include/demo/client.h",
		r#"#ifndef DEMO_CLIENT_H
#define DEMO_CLIENT_H
#include <demo/dispatch.h>
static inline int use_schedule(struct item *item) { return schedule(item); }
#endif
"#,
	);
	write(
		root,
		"src/client.c",
		"#include <demo/client.h>\nvoid run(void) { action(false); incomplete(1); incompatible(2); }\n",
	);
	write(
		root,
		"src/dispatch.c",
		"#include <demo/dispatch.h>\nint enqueue(struct item *value) { return value != 0; }\nvoid action(bool ready) { (void)ready; }\nlong incompatible(int value) { return value; }\n",
	);
	write(
		root,
		"src/unrelated.c",
		"struct item;\nint enqueue(struct item *item) { return item == 0; }\nstatic int schedule(struct item *item) { return item == 0; }\n",
	);
}

#[test]
fn c_angle_headers_link_visible_inline_functions_and_common_prototypes() {
	let dir = tempfile::tempdir().unwrap();
	let root = dir.path();
	common_header_fixture(root);
	let schedule = callable_uri(root, "schedule", "include/demo/dispatch.h");
	let enqueue = callable_uri(root, "enqueue", "src/dispatch.c");
	let action = callable_uri(root, "action", "src/dispatch.c");
	let prototype = callable_uri(root, "enqueue", "include/demo/dispatch.h");
	assert_eq!(
		usage_locations(root, &schedule),
		["include/demo/client.h:L4"]
	);
	assert_eq!(
		usage_locations(root, &enqueue),
		["include/demo/dispatch.h:L9"]
	);
	assert_eq!(usage_locations(root, &action), ["src/client.c:L2"]);
	assert_eq!(
		query(root, &format!("symbol.detail uri:{prototype}"))["symbol"]["line_range"][0],
		5
	);
	assert!(usage_locations(root, &callable_uri(root, "enqueue", "src/unrelated.c")).is_empty());
	assert!(usage_locations(root, &callable_uri(root, "schedule", "src/unrelated.c")).is_empty());
	assert!(
		usage_locations(root, &callable_uri(root, "incompatible", "src/dispatch.c")).is_empty()
	);
	assert_eq!(
		usage_locations(
			root,
			&callable_uri(root, "incomplete", "include/demo/dispatch.h")
		),
		["src/client.c:L2"]
	);
	assert_eq!(
		usage_locations(
			root,
			&callable_uri(root, "incompatible", "include/demo/dispatch.h")
		),
		["src/client.c:L2"]
	);
}

#[test]
fn c_two_bodies_of_an_included_prototype_keep_the_call_on_the_header() {
	let dir = tempfile::tempdir().unwrap();
	let root = dir.path();
	common_header_fixture(root);
	write(
		root,
		"src/second.c",
		"#include <demo/dispatch.h>\nint enqueue(struct item *item) { return item == 0; }\n",
	);
	let second = callable_uri(root, "enqueue", "src/second.c");
	let enqueue = callable_uri(root, "enqueue", "src/dispatch.c");
	let prototype = callable_uri(root, "enqueue", "include/demo/dispatch.h");
	assert_eq!(
		(
			usage_locations(root, &enqueue),
			usage_locations(root, &second)
		),
		(Vec::<String>::new(), Vec::<String>::new())
	);
	assert_eq!(
		usage_locations(root, &prototype),
		["include/demo/dispatch.h:L9"]
	);
}

#[test]
fn c_included_function_like_macro_prevents_promoting_an_external_body() {
	let dir = tempfile::tempdir().unwrap();
	let root = dir.path();
	write(root, "include/demo/callable.h", "int step(int value);\n");
	write(
		root,
		"include/demo/macro.h",
		"#define step(value) ((value) + 42)\n",
	);
	write(
		root,
		"src/dispatch.c",
		"#include <demo/callable.h>\nint step(int value) { return value + 1; }\n",
	);
	write(
		root,
		"src/client.c",
		"#include <demo/callable.h>\n#include <demo/macro.h>\nint run(int value) { return step(value); }\n",
	);
	assert!(usage_locations(root, &callable_uri(root, "step", "src/dispatch.c")).is_empty());
}

#[test]
fn c_explicit_include_root_has_priority_over_the_conventional_root() {
	let dir = tempfile::tempdir().unwrap();
	let root = dir.path();
	common_header_fixture(root);
	write(
		root,
		"alt/demo/dispatch.h",
		"struct item;\nstatic inline int schedule(struct item *item) { return item == 0; }\n",
	);
	write(root, "Makefile", "CPPFLAGS += -I alt -I include\n");
	let schedule = callable_uri(root, "schedule", "include/demo/dispatch.h");
	assert!(usage_locations(root, &schedule).is_empty());
	assert_eq!(
		usage_locations(root, &callable_uri(root, "schedule", "alt/demo/dispatch.h")),
		["include/demo/client.h:L4"]
	);
}

#[test]
fn c_explicit_include_in_a_blocked_group_cannot_supply_a_call() {
	let dir = tempfile::tempdir().unwrap();
	let root = dir.path();
	write(
		root,
		".code-moniker.toml",
		"[[workspace.source_group]]\nroots = [\"src\", \"visible\"]\n[[workspace.source_group]]\nroots = [\"blocked\"]\n",
	);
	write(
		root,
		"src/main.c",
		"#include \"../visible/dispatch.h\"\n#include \"../blocked/dispatch.h\"\nint run(int x) { return schedule(x); }\n",
	);
	write(
		root,
		"visible/dispatch.h",
		"static inline int schedule(int x) { return x + 1; }\n",
	);
	write(
		root,
		"blocked/dispatch.h",
		"static inline int schedule(int x) { return x + 2; }\n",
	);
	assert_eq!(
		usage_locations(root, &callable_uri(root, "schedule", "visible/dispatch.h")),
		["src/main.c:L3"]
	);
	assert!(
		usage_locations(root, &callable_uri(root, "schedule", "blocked/dispatch.h")).is_empty()
	);
}

#[test]
fn c_two_included_inline_bodies_remain_ambiguous() {
	let dir = tempfile::tempdir().unwrap();
	let root = dir.path();
	write(
		root,
		"include/demo/first.h",
		"static inline int step(int x) { return x + 1; }\n",
	);
	write(
		root,
		"include/demo/second.h",
		"static inline int step(int x) { return x + 2; }\n",
	);
	write(
		root,
		"src/main.c",
		"#if CONFIG_A\n#include <demo/first.h>\n#else\n#include <demo/second.h>\n#endif\nint run(int x) { return step(x); }\n",
	);
	for path in ["include/demo/first.h", "include/demo/second.h"] {
		assert!(usage_locations(root, &callable_uri(root, "step", path)).is_empty());
	}
}

#[test]
fn c_usages_link_a_local_prototype_to_a_unique_compatible_external_definition() {
	let project = tempfile::tempdir().unwrap();
	let root = project.path();
	fs::write(
		root.join("consumer.c"),
		r#"#include "helpers.h"
int scale(int input);
int missing(int value);
int private_other_file(int value);
int ambiguous(int value);
int incompatible(int value);
int incompatible_return(int value);
int opaque_competitor(int value);
char *plain_pointer(char *);
static int helper(int value);
static int helper(int value) { return value; }
int adjust(int x) {
  return scale(x) + missing(x) + private_other_file(x) + ambiguous(x) +
    incompatible(x) + incompatible_return(x) + opaque_competitor(x) +
    helper(x) + inline_helper(x) + (plain_pointer(0) != 0);
}
"#,
	)
	.unwrap();
	fs::write(
		root.join("providers.c"),
		r#"int scale(int value) { return value * 2; }
static int private_other_file(int value) { return value; }
int ambiguous(int value) { return value; }
int incompatible(long value) { return (int)value; }
long incompatible_return(int value) { return value; }
int opaque_competitor(int value) { return value; }
char *plain_pointer(char *value) { return value; }
int helper(int value) { return value * 2; }
"#,
	)
	.unwrap();
	fs::write(
		root.join("alternative.c"),
		"int ambiguous(int value) { return value + 1; }\ntypedef int number;\nnumber opaque_competitor(int value) { return value; }\n",
	)
	.unwrap();
	fs::write(
		root.join("helpers.h"),
		"static inline int inline_helper(int value) { return value; }\n",
	)
	.unwrap();

	let search = query(root, "symbol.search lang:c shape:callable limit:100");
	let rows = search["rows"].as_array().unwrap();
	let find = |file: &str, function: &str| {
		rows.iter()
			.find(|row| {
				row["file"] == file
					&& row["name"].as_str().unwrap().split_once('(').unwrap().0 == function
			})
			.unwrap_or_else(|| panic!("missing {function} in {file}: {rows:#?}"))
	};
	let assert_usage = |file: &str, function: &str, call_line: Option<u64>| {
		let target = find(file, function);
		let usage = query(root, &format!("symbol.usages uri:{}", target["uri"]));
		assert_eq!(
			usage["total"],
			usize::from(call_line.is_some()),
			"{file} {function}: {usage}"
		);
		if let Some(line) = call_line {
			assert_eq!(usage["rows"][0]["kind"], "calls");
			assert_eq!(usage["rows"][0]["file"], "consumer.c");
			assert_eq!(usage["rows"][0]["line_range"][0], line);
		}
	};
	let prototype = find("consumer.c", "scale");
	let detail = query(root, &format!("symbol.detail uri:{}", prototype["uri"]));
	assert_eq!(detail["symbol"]["file"], "consumer.c");
	assert_eq!(detail["symbol"]["line_range"][0], 2);
	assert_usage("providers.c", "scale", Some(13));
	assert_usage("consumer.c", "scale", None);
	assert_usage("providers.c", "plain_pointer", Some(15));
	assert_usage("consumer.c", "missing", Some(13));
	assert_usage("consumer.c", "private_other_file", Some(13));
	assert_usage("providers.c", "private_other_file", None);
	assert_usage("consumer.c", "ambiguous", Some(13));
	assert_usage("providers.c", "ambiguous", None);
	assert_usage("alternative.c", "ambiguous", None);
	assert_usage("consumer.c", "incompatible", Some(14));
	assert_usage("consumer.c", "incompatible_return", Some(14));
	assert_usage("consumer.c", "opaque_competitor", Some(14));
	assert_usage("consumer.c", "helper", Some(15));
	assert_usage("providers.c", "helper", None);
	assert_usage("helpers.h", "inline_helper", Some(15));
}
