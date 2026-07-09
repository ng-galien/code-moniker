import { ChangeReviewFile, ChangeReviewResult, ChangeReviewSymbol } from "../daemon/model";

export type ChangeTreeNode = ChangeFileNode | ChangeSymbolNode | ChangeInfoNode;

// A changed file row from the semantic review (disposition, coverage).
export interface ChangeFileNode {
	kind: "file";
	file: ChangeReviewFile;
	review: ChangeReviewResult;
}

// A per-symbol change fact under its file.
export interface ChangeSymbolNode {
	kind: "symbolChange";
	change: ChangeReviewSymbol;
}

export interface ChangeInfoNode {
	kind: "info";
	label: string;
}

export function changeFilePath(file: ChangeReviewFile): string {
	return file.new_path ?? file.old_path ?? "<unknown>";
}

export function changeSymbolPath(change: ChangeReviewSymbol): string | undefined {
	return change.new?.file ?? change.old?.file;
}

export function changeSymbolName(change: ChangeReviewSymbol): string {
	return change.new?.name ?? change.old?.name ?? "<unknown>";
}
