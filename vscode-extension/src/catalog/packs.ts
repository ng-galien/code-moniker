import { runLearnIndex, runLearnPack } from "../cli/facade";
import { langByTomlSection } from "../shared/languages";

// Sample packs served by the CLI (`code-moniker rules learn`). The CLI is the
// single source of truth: names, blurbs, and languages come from the scenario
// front matter, never from a hardcoded list.
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
	const result = await runLearnIndex();
	if (!result.ok) {
		return result;
	}
	const packs = result.report.samples
		.filter((sample) => sample.published !== false)
		.map((sample) => ({
			name: sample.name,
			langId: sample.lang ? langByTomlSection(sample.lang)?.id : undefined,
			blurb: sample.blurb?.trim() || `Rule pack \`${sample.name}\`.`,
			document: sample.document,
		}));
	return { ok: true, packs };
}

export type PackScenarioResult =
	| { ok: true; document: string }
	| { ok: false; error: string };

export async function loadPackScenario(name: string): Promise<PackScenarioResult> {
	const result = await runLearnPack(name);
	if (!result.ok) {
		return { ok: false, error: result.error };
	}
	const document = result.report.samples[0]?.document;
	if (!document) {
		return {
			ok: false,
			error: `Sample \`${name}\` has no scenario document — update the code-moniker CLI.`,
		};
	}
	return { ok: true, document };
}
