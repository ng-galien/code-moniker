import * as vscode from "vscode";

import { CmnbCell, CmnbDocument } from "../notebook/model";
import { LESSONS } from "./data";

export type LessonCellsResult =
	| { ok: true; title: string; cells: CmnbCell[] }
	| { ok: false; error: string };

export async function loadLessonCells(
	context: vscode.ExtensionContext,
	id: string,
): Promise<LessonCellsResult> {
	const entry = LESSONS.find((lesson) => lesson.id === id);
	if (!entry) {
		return { ok: false, error: `Unknown lesson "${id}".` };
	}
	const uri = vscode.Uri.joinPath(context.extensionUri, "notebooks", entry.file);
	try {
		const bytes = await vscode.workspace.fs.readFile(uri);
		const doc = JSON.parse(new TextDecoder().decode(bytes)) as CmnbDocument;
		if (!Array.isArray(doc.cells)) {
			return { ok: false, error: `${entry.file} does not contain cells.` };
		}
		return { ok: true, title: entry.title, cells: doc.cells };
	} catch (err) {
		return {
			ok: false,
			error: `Could not open ${entry.file}: ${(err as Error).message}`,
		};
	}
}
