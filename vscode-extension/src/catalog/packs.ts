import { langByTomlSection } from "../shared/languages";

import architecture from "../../../samples/catalog/architecture.cm.md";
import cleanArchitecture from "../../../samples/catalog/clean-architecture.cm.md";
import csharp from "../../../samples/catalog/csharp.cm.md";
import fowlerEaa from "../../../samples/catalog/fowler-eaa.cm.md";
import fowlerRefactoring from "../../../samples/catalog/fowler-refactoring.cm.md";
import go from "../../../samples/catalog/go.cm.md";
import java from "../../../samples/catalog/java.cm.md";
import javaLayerBoundaries from "../../../samples/catalog/java-layer-boundaries.cm.md";
import python from "../../../samples/catalog/python.cm.md";
import rust from "../../../samples/catalog/rust.cm.md";
import rustNaming from "../../../samples/catalog/rust-naming.cm.md";
import sql from "../../../samples/catalog/sql.cm.md";
import testGuardrails from "../../../samples/catalog/test-guardrails.cm.md";
import typescript from "../../../samples/catalog/typescript.cm.md";

const CATALOG_DOCUMENTS = [
	architecture,
	cleanArchitecture,
	csharp,
	fowlerEaa,
	fowlerRefactoring,
	go,
	java,
	javaLayerBoundaries,
	python,
	rust,
	rustNaming,
	sql,
	testGuardrails,
	typescript,
];

// Sample packs are built directly from the repository's scenario documents.
// The CLI remains only the execution engine, not the catalog source.
export interface PackEntry {
	name: string;
	langId?: string;
	blurb: string;
	/** Full scenario Markdown document (multi-file layout + rules + expects). */
	document?: string;
}

export type PackIndexResult =
	| { ok: true; packs: PackEntry[] }
	| { ok: false; error: string };

export async function loadPackIndex(): Promise<PackIndexResult> {
	const packs = CATALOG_DOCUMENTS.map(packEntry).filter((pack) => pack !== undefined);
	return { ok: true, packs };
}

function packEntry(document: string): PackEntry | undefined {
	const meta = frontMatter(document);
	if (!meta.name || meta.published === "false") {
		return undefined;
	}
	return {
		name: meta.name,
		langId: meta.lang ? langByTomlSection(meta.lang)?.id : undefined,
		blurb: meta.blurb?.trim() || `Scenario \`${meta.name}\`.`,
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
