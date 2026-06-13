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
import learnBasics from "../../../samples/learn/basics.cm.md";
import learnCollections from "../../../samples/learn/collections.cm.md";
import learnMetrics from "../../../samples/learn/metrics.cm.md";
import learnPaths from "../../../samples/learn/paths.cm.md";
import learnProfiles from "../../../samples/learn/profiles.cm.md";
import learnRefs from "../../../samples/learn/refs.cm.md";

const CATALOG_DOCUMENTS = [
	{ category: "sample" as const, document: architecture },
	{ category: "sample" as const, document: cleanArchitecture },
	{ category: "sample" as const, document: csharp },
	{ category: "sample" as const, document: fowlerEaa },
	{ category: "sample" as const, document: fowlerRefactoring },
	{ category: "sample" as const, document: go },
	{ category: "sample" as const, document: java },
	{ category: "sample" as const, document: javaLayerBoundaries },
	{ category: "sample" as const, document: python },
	{ category: "sample" as const, document: rust },
	{ category: "sample" as const, document: rustNaming },
	{ category: "sample" as const, document: sql },
	{ category: "sample" as const, document: testGuardrails },
	{ category: "sample" as const, document: typescript },
	{ category: "learn" as const, document: learnBasics },
	{ category: "learn" as const, document: learnCollections },
	{ category: "learn" as const, document: learnMetrics },
	{ category: "learn" as const, document: learnPaths },
	{ category: "learn" as const, document: learnProfiles },
	{ category: "learn" as const, document: learnRefs },
];

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
