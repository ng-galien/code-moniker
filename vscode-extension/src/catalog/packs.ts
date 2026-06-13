import { langByTomlSection } from "../shared/languages";
import { CATALOG_DOCUMENTS } from "code-moniker-sample-packs";

// Sample packs are built directly from the repository's scenario documents.
// The CLI remains only the execution engine, not the catalog source.
export interface PackEntry {
	name: string;
	title: string;
	category: "learn" | "sample";
	langId?: string;
	blurb: string;
	/** Full scenario Markdown document (multi-file layout + rules + expects). */
	document?: string;
}

export type PackIndexResult =
	| { ok: true; packs: PackEntry[] }
	| { ok: false; error: string };

export async function loadPackIndex(): Promise<PackIndexResult> {
	const packs = CATALOG_DOCUMENTS.map(({ category, document }) =>
		packEntry(category, document),
	).filter((pack) => pack !== undefined);
	return { ok: true, packs };
}

function packEntry(category: "learn" | "sample", document: string): PackEntry | undefined {
	const meta = frontMatter(document);
	if (!meta.name || meta.published === "false") {
		return undefined;
	}
	return {
		name: meta.name,
		title: meta.title?.trim() || `${meta.name} scenario`,
		category,
		langId: meta.lang ? langByTomlSection(meta.lang)?.id : undefined,
		blurb: meta.blurb?.trim() || meta.summary?.trim() || `Scenario \`${meta.name}\`.`,
		document,
	};
}

function frontMatter(document: string): Record<string, string> {
	const match = /^---\n([\s\S]*?)\n---/.exec(document);
	if (!match) {
		return {};
	}
	const meta: Record<string, string> = {};
	for (const line of match[1].split("\n")) {
		const pair = /^([A-Za-z_][A-Za-z0-9_]*):\s*(.*)$/.exec(line.trim());
		if (pair) {
			meta[pair[1]] = pair[2].trim();
		}
	}
	return meta;
}
