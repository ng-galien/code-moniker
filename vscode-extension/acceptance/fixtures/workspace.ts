import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";

export function seedAcceptanceWorkspace(root: string): void {
	mkdirSync(join(root, "src"), { recursive: true });
	writeFileSync(
		join(root, "src", "lib.rs"),
		[
			"pub struct Widget {",
			"\tpub size: u32,",
			"}",
			"",
			"impl Widget {",
			"\tpub fn new() -> Self {",
			"\t\tWidget { size: 0 }",
			"\t}",
			"",
			"\tpub fn grow(&mut self) {",
			"\t\tself.size += 1;",
			"\t}",
			"}",
			"",
			"pub fn build_widget() -> Widget {",
			"\tWidget::new()",
			"}",
			"",
			"pub fn dep_a() {}",
			"pub fn dep_b() {}",
			"pub fn dep_c() {}",
			"pub fn dep_d() {}",
			"pub fn dep_e() {}",
			"pub fn dep_f() {}",
			"pub fn dep_g() {}",
			"pub fn dep_h() {}",
			"",
			"pub fn fan_out() {",
			"\tdep_a(); dep_b(); dep_c(); dep_d();",
			"\tdep_e(); dep_f(); dep_g(); dep_h();",
			"}",
			"",
			"pub fn fan_in_one() { fan_out(); }",
			"pub fn fan_in_two() { fan_out(); }",
			"",
			"pub fn DoThing() {",
			"\tlet _ = build_widget();",
			"}",
			"",
		].join("\n"),
	);
	writeFileSync(
		join(root, ".code-moniker.toml"),
		[
			"default_rules = false",
			"",
			"[[rust.fn.where]]",
			'id = "function-snake-case"',
			'expr = "name =~ ^[a-z][a-z0-9_]*$"',
			'severity = "warn"',
			'message = "Function `{name}` should be snake_case."',
			"",
		].join("\n"),
	);
}
