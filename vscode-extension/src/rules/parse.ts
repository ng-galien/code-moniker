// Lightweight scanner for .code-moniker.toml / *.fragment.toml: lists rule
// blocks with their scope, id, severity, source line, and raw TOML. The CLI is
// the source of truth for validity; this only powers the tree and navigation.

import { langByTomlSection } from "../shared/languages";

export interface RuleEntry {
	/** Scope without the trailing `.where`, e.g. `rust.fn`, `ts.shape.callable`, `refs`. */
	scope: string;
	id: string;
	severity: string;
	/** 0-indexed line of the `[[...where]]` header. */
	line: number;
	/** Raw TOML of this rule block. */
	blockText: string;
	/** Sample language id inferred from the scope section, if any. */
	sampleLang?: string;
}

export interface ParsedRuleFile {
	rules: RuleEntry[];
	aliases: string[];
	profiles: string[];
}

const HEADER = /^\s*\[\[(.+?)\.where\]\]\s*$/;
const TABLE = /^\s*\[/;
const ALIASES = /^\s*\[aliases\]\s*$/;
const PROFILE = /^\s*\[profiles\.([^\]]+)\]\s*$/;

export function parseRuleFile(text: string): ParsedRuleFile {
	const lines = text.split("\n");
	const rules: RuleEntry[] = [];
	const aliases: string[] = [];
	const profiles: string[] = [];

	for (let i = 0; i < lines.length; i++) {
		const profileMatch = PROFILE.exec(lines[i]);
		if (profileMatch) {
			profiles.push(profileMatch[1]);
			continue;
		}
		if (ALIASES.test(lines[i])) {
			for (let j = i + 1; j < lines.length && !TABLE.test(lines[j]); j++) {
				const alias = /^\s*([A-Za-z0-9_]+)\s*=/.exec(lines[j]);
				if (alias) {
					aliases.push(alias[1]);
				}
			}
			continue;
		}
		const header = HEADER.exec(lines[i]);
		if (!header) {
			continue;
		}
		const scope = header[1];
		let end = i + 1;
		while (end < lines.length && !TABLE.test(lines[end])) {
			end++;
		}
		const block = lines.slice(i, end);
		const id = field(block, ID_FIELD) ?? `${scope}.where[${rules.length}]`;
		const severity = field(block, SEVERITY_FIELD) ?? "error";
		const section = scope.split(".")[0];
		rules.push({
			scope,
			id,
			severity,
			line: i,
			blockText: block.join("\n").trimEnd() + "\n",
			sampleLang: langByTomlSection(section)?.id,
		});
	}
	return { rules, aliases, profiles };
}

const ID_FIELD = /^\s*id\s*=\s*["'](.+?)["']\s*$/;
const SEVERITY_FIELD = /^\s*severity\s*=\s*["'](.+?)["']\s*$/;

function field(block: string[], re: RegExp): string | undefined {
	for (const line of block) {
		const m = re.exec(line);
		if (m) {
			return m[1];
		}
	}
	return undefined;
}
