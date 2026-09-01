// Maps scenario metadata and syntax fences to VS Code languages. Rule sections
// and `rules eval` tags are optional because syntax-only languages can exist.

export interface LangDef {
	/** Identifier stored in scenario cell metadata. */
	id: string;
	/** VSCode languageId used to syntax-highlight the sample cell. */
	vscodeId: string;
	/** Tag passed to `code-moniker rules eval --lang`, when graph evaluation exists. */
	cliTag?: string;
	/** TOML rule table section, e.g. `[[rust.fn.where]]` uses "rust". */
	tomlSection?: string;
	/** Human label for pickers. */
	label: string;
}

export const LANGS: LangDef[] = [
	{ id: "rust", vscodeId: "rust", cliTag: "rs", tomlSection: "rust", label: "Rust" },
	{
		id: "typescript",
		vscodeId: "typescript",
		cliTag: "ts",
		tomlSection: "ts",
		label: "TypeScript",
	},
	{ id: "tsx", vscodeId: "typescriptreact", cliTag: "tsx", tomlSection: "tsx", label: "TSX" },
	{
		id: "javascript",
		vscodeId: "javascript",
		cliTag: "js",
		tomlSection: "js",
		label: "JavaScript",
	},
	{ id: "jsx", vscodeId: "javascriptreact", cliTag: "jsx", tomlSection: "jsx", label: "JSX" },
	{ id: "python", vscodeId: "python", cliTag: "python", tomlSection: "python", label: "Python" },
	{ id: "go", vscodeId: "go", cliTag: "go", tomlSection: "go", label: "Go" },
	{ id: "java", vscodeId: "java", cliTag: "java", tomlSection: "java", label: "Java" },
	{ id: "c", vscodeId: "c", cliTag: "c", tomlSection: "c", label: "C" },
	{ id: "csharp", vscodeId: "csharp", cliTag: "cs", tomlSection: "cs", label: "C#" },
	{ id: "sql", vscodeId: "sql", cliTag: "sql", tomlSection: "sql", label: "SQL" },
	{
		id: "plpgsql",
		vscodeId: "sql",
		label: "PL/pgSQL",
	},
];

export function langById(id: string): LangDef | undefined {
	return LANGS.find((lang) => lang.id === id);
}

export function langByVscodeId(id: string): LangDef | undefined {
	return LANGS.find((lang) => lang.vscodeId === id);
}

export function langByCliTag(tag: string): LangDef | undefined {
	return LANGS.find((lang) => lang.cliTag === tag);
}

/** Resolves scenario metadata or a source fence, including syntax-only tags. */
export function langByMetadataTag(tag: string): LangDef | undefined {
	return LANGS.find((lang) =>
		lang.id === tag || lang.cliTag === tag || lang.tomlSection === tag,
	);
}

/** Resolves a TOML rule section (e.g. "rust", "ts") to its language. */
export function langByTomlSection(section: string): LangDef | undefined {
	return LANGS.find((lang) => lang.tomlSection === section);
}

export function vscodeLanguageForScenarioFence(fence: string | undefined): string {
	switch (fence) {
		case "tsx":
			return "typescriptreact";
		case "jsx":
			return "javascriptreact";
		case "ts":
			return "typescript";
		case "js":
			return "javascript";
		default:
			return langByMetadataTag(fence ?? "")?.vscodeId ?? fence ?? "plaintext";
	}
}

export function scenarioControllerLanguages(): string[] {
	return [...new Set(["cmrule-toml", "plaintext", ...LANGS.map((lang) => lang.vscodeId)])];
}
