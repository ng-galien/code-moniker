import { runLearnPack } from "../cli/facade";

export type PackContentResult =
	| { ok: true; content: string }
	| { ok: false; error: string };

export async function loadPackContent(name: string): Promise<PackContentResult> {
	const result = await runLearnPack(name);
	if (!result.ok) {
		return { ok: false, error: result.error };
	}
	return { ok: true, content: result.report.samples[0]?.content ?? "" };
}
