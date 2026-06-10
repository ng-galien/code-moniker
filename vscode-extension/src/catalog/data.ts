// Curated catalog of check-DSL concepts. Each entry is a runnable lesson:
// an explanation, a sample, and a real rule fragment. Verified to produce a
// violation against its sample (see scripts and the e2e check).

export interface ConceptEntry {
	id: string;
	title: string;
	blurb: string;
	/** Sample language id. */
	langId: string;
	sample: string;
	ruleToml: string;
}

export const CONCEPTS: ConceptEntry[] = [
	{
		id: "scopes",
		title: "Scopes & kinds",
		blurb:
			"A rule's table header chooses what it runs on: `[[rust.fn.where]]` for Rust functions, `[[ts.class.where]]` for TS classes, `[[<lang>.shape.callable.where]]` for any callable, or `[[refs.where]]` for references.",
		langId: "rust",
		sample: "fn parse_input() {}\nfn DoThing() {}\n",
		ruleToml:
			'default_rules = false\n\n[[rust.fn.where]]\nid        = "snake-case"\nexpr      = "name =~ ^[a-z][a-z0-9_]*$"\nmessage   = "Function `{name}` should be snake_case."\n',
	},
	{
		id: "comparisons",
		title: "Fields & comparisons",
		blurb:
			"Fields (`name`, `kind`, `visibility`, `lines`, …) describe a symbol. Compare with `=`, `!=`, `<`, `<=`, `>`, `>=`. A symbol violates the rule when the expression is false for it.",
		langId: "rust",
		sample: "pub fn good() {}\npub fn Bad() {}\n",
		ruleToml:
			'default_rules = false\n\n[[rust.fn.where]]\nid        = "lowercase"\nexpr      = "name =~ ^[a-z]"\nmessage   = "`{name}` should start lowercase."\n',
	},
	{
		id: "regex",
		title: "Regex match: =~ and !~",
		blurb:
			"`=~` matches a regex, `!~` is the negation. The right-hand side is an unquoted regex token, e.g. `name !~ ^(helper|do_thing)$`.",
		langId: "rust",
		sample: "fn extract_symbols() {}\nfn helper() {}\n",
		ruleToml:
			'default_rules = false\n\n[[rust.fn.where]]\nid        = "no-placeholders"\nexpr      = "name !~ ^(helper|do_thing|manage|process)$"\nmessage   = "`{name}` is a placeholder; name the behaviour."\n',
	},
	{
		id: "boolean",
		title: "Boolean logic: AND OR NOT",
		blurb:
			"Combine conditions with `AND`, `OR`, `NOT`, and parentheses. Precedence is `NOT` > `AND` > `OR`.",
		langId: "rust",
		sample: "fn keep() {}\nfn tmp() {}\n",
		ruleToml:
			'default_rules = false\n\n[[rust.fn.where]]\nid        = "no-temp"\nexpr      = "NOT (name = \'tmp\' OR name = \'temp\')"\nmessage   = "`{name}` looks temporary."\n',
	},
	{
		id: "implication",
		title: "Implication: =>",
		blurb:
			"`A => B` means *when A holds, B must hold too*. Perfect for constraining only part of the surface: here, only public functions are checked.",
		langId: "rust",
		sample: "pub fn Good() {}\nfn helper() {}\n",
		ruleToml:
			'default_rules = false\n\n[[rust.fn.where]]\nid        = "public-lowercase"\nexpr      = "visibility = \'public\' => name =~ ^[a-z]"\nmessage   = "Public function `{name}` should start lowercase."\nrationale = "Private helpers are exempt; only the public API is constrained."\n',
	},
	{
		id: "lines",
		title: "Size: the lines field",
		blurb:
			"`lines` is the symbol's line span. `shape:callable` matches functions and methods across languages. `severity = \"warn\"` reports without failing a gate.",
		langId: "rust",
		sample: "fn short() {\n    let _ = 1;\n}\n\nfn long() {\n    let a = 1;\n    let b = 2;\n    let c = 3;\n    let d = 4;\n    let _ = a + b + c + d;\n}\n",
		ruleToml:
			'default_rules = false\n\n[[rust.shape.callable.where]]\nid        = "max-lines"\nexpr      = "lines <= 4"\nseverity  = "warn"\nmessage   = "{kind} `{name}` is {value} lines (cap {expected})."\n',
	},
	{
		id: "quantifiers",
		title: "Quantifiers: any / all / none",
		blurb:
			"Quantify over a child domain such as `param`: `none(param, …)`, `any(param, …)`, `all(param, …)`. Here no parameter may start with an underscore.",
		langId: "rust",
		sample: "fn clean(value: i32) {}\nfn noisy(_unused: i32, value: i32) {}\n",
		ruleToml:
			'default_rules = false\n\n[[rust.fn.where]]\nid        = "no-underscore-params"\nexpr      = "none(param, name =~ ^_)"\nmessage   = "`{name}` has an underscore-prefixed parameter."\n',
	},
	{
		id: "count",
		title: "Counting children: count()",
		blurb:
			"`count(param)` counts a symbol's parameters; `count(method)` its methods. Compare the count against a budget.",
		langId: "rust",
		sample: "fn small(a: i32, b: i32) {}\nfn big(a: i32, b: i32, c: i32, d: i32, e: i32) {}\n",
		ruleToml:
			'default_rules = false\n\n[[rust.fn.where]]\nid        = "arity"\nexpr      = "count(param) <= 3"\nmessage   = "`{name}` takes {value} parameters (max {expected})."\nrationale = "High arity often hides a missing parameter struct."\n',
	},
	{
		id: "aliases",
		title: "Aliases: reuse predicates",
		blurb:
			"`[aliases]` names a reusable predicate; reference it with `$name`. Aliases keep a rule pack DRY — exactly as in a project `.code-moniker.toml`.",
		langId: "rust",
		sample: "pub fn _Hidden() {}\npub fn ok() {}\n",
		ruleToml:
			'default_rules = false\n\n[aliases]\npublic_fn = "visibility = \'public\'"\n\n[[rust.fn.where]]\nid        = "public-clean-name"\nexpr      = "$public_fn => name =~ ^[a-z]"\nmessage   = "Public function `{name}` should start lowercase."\n',
	},
	{
		id: "severity-messages",
		title: "Severity, message & rationale",
		blurb:
			"`severity` is `error` (default) or `warn`. `message` is the actionable text shown per violation and supports `{kind} {name} {value} {expected}` templates. `rationale` records the why.",
		langId: "rust",
		sample: "pub struct RiskPolicy {}\npub struct http_client {}\n",
		ruleToml:
			'default_rules = false\n\n[[rust.struct.where]]\nid        = "pascal-types"\nexpr      = "name =~ ^[A-Z]"\nseverity  = "warn"\nmessage   = "Type `{name}` should be PascalCase."\nrationale = "Type names are PascalCase across the public API."\n',
	},
	{
		id: "typescript-naming",
		title: "TypeScript: class & function naming",
		blurb:
			"The same DSL works for every language; only the scope section changes (`ts`, `python`, `go`, `java`, `cs`, `sql`).",
		langId: "typescript",
		sample: "class RiskPolicy {}\nclass userSession {}\n",
		ruleToml:
			'default_rules = false\n\n[[ts.class.where]]\nid        = "pascal-class"\nexpr      = "name =~ ^[A-Z][A-Za-z0-9]*$"\nmessage   = "Class `{name}` should be PascalCase."\n',
	},
];

export interface LessonEntry {
	id: string;
	title: string;
	file: string;
	blurb: string;
	langId?: string;
	tags: string[];
}

export const LESSONS: LessonEntry[] = [
	{
		id: "dsl-basics",
		title: "Check DSL basics",
		file: "Check DSL basics.cmnb",
		blurb: "A guided walkthrough of scopes, fields, boolean logic, aliases, and counts.",
		langId: "rust",
		tags: ["dsl", "rules", "rust"],
	},
	{
		id: "rust",
		title: "Rust rules",
		file: "Rust rules.cmnb",
		blurb: "Runnable Rust naming, public API, size, and placeholder-name lessons.",
		langId: "rust",
		tags: ["rust", "naming", "size"],
	},
	{
		id: "typescript",
		title: "TypeScript rules",
		file: "TypeScript rules.cmnb",
		blurb: "Runnable TypeScript class, function, and callable-size lessons.",
		langId: "typescript",
		tags: ["typescript", "naming", "size"],
	},
	{
		id: "java",
		title: "Java rules",
		file: "Java rules.cmnb",
		blurb: "Runnable Java class, method, and method-size lessons.",
		langId: "java",
		tags: ["java", "naming", "size"],
	},
];

// Sample packs are served by the CLI (`code-moniker rules learn`); see
// catalog/packs.ts. Their names, languages, and blurbs come from the scenario
// documents in the main repository's `samples/` directory.
