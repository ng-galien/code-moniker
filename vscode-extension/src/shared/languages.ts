// Maps a scenario sample language to its VSCode editor language id and the
// code-moniker CLI tag accepted by `rules eval --lang`.

export interface LangDef {
	/** Identifier stored in scenario cell metadata. */
	id: string;
	/** VSCode languageId used to syntax-highlight the sample cell. */
	vscodeId: string;
	/** Tag passed to `code-moniker rules eval --lang`. */
	cliTag: string;
	/** TOML rule table section, e.g. `[[rust.fn.where]]` uses "rust". */
	tomlSection: string;
	/** Human label for pickers. */
	label: string;
}

export const LANGS: LangDef[] = [
	{ id: "rust", vscodeId: "rust", cliTag: "rs", tomlSection: "rust", label: "Rust" },
	{ id: "typescript", vscodeId: "typescript", cliTag: "ts", tomlSection: "ts", label: "TypeScript" },
	{ id: "python", vscodeId: "python", cliTag: "python", tomlSection: "python", label: "Python" },
	{ id: "go", vscodeId: "go", cliTag: "go", tomlSection: "go", label: "Go" },
	{ id: "java", vscodeId: "java", cliTag: "java", tomlSection: "java", label: "Java" },
	{ id: "csharp", vscodeId: "csharp", cliTag: "cs", tomlSection: "cs", label: "C#" },
	{ id: "sql", vscodeId: "sql", cliTag: "sql", tomlSection: "sql", label: "SQL" },
];

export function langById(id: string): LangDef | undefined {
	return LANGS.find((lang) => lang.id === id);
}

export function langByVscodeId(id: string): LangDef | undefined {
	return LANGS.find((lang) => lang.vscodeId === id);
}

/** Resolves a TOML rule section (e.g. "rust", "ts") to its language. */
export function langByTomlSection(section: string): LangDef | undefined {
	return LANGS.find((lang) => lang.tomlSection === section);
}
