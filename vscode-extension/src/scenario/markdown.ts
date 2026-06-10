// Parse/serialize the scenario Markdown subset (see docs/check-scenarios.md):
// optional front matter, top-level backtick fences whose info string carries a
// `cm:` token. Everything else is prose. Serialization is canonical — blocks
// separated by a single blank line — so notebook saves stay diff-friendly.

import { ScenarioCell, ScenarioDocument } from "./model";

export function parseScenarioMarkdown(text: string): ScenarioDocument {
	const lines = text.split("\n");
	const document: ScenarioDocument = { cells: [] };
	let cursor = consumeFrontMatter(lines, document);
	let prose: string[] = [];
	while (cursor < lines.length) {
		const fence = fenceLength(lines[cursor]);
		const block = fence ? classifyInfo(lines[cursor]) : undefined;
		if (!fence || !block) {
			prose.push(lines[cursor]);
			cursor += 1;
			continue;
		}
		flushProse(prose, document.cells);
		prose = [];
		const close = findClosingFence(lines, cursor + 1, fence);
		const value = joinBlock(lines.slice(cursor + 1, close));
		document.cells.push(blockCell(block, value));
		cursor = close + 1;
	}
	flushProse(prose, document.cells);
	return document;
}

export function serializeScenarioMarkdown(document: ScenarioDocument): string {
	const blocks: string[] = [];
	if (document.frontMatter !== undefined) {
		blocks.push(`---\n${ensureTrailingNewline(document.frontMatter)}---`);
	}
	for (const cell of document.cells) {
		blocks.push(serializeCell(cell));
	}
	return blocks.join("\n\n") + "\n";
}

interface BlockInfo {
	cmType: "rules" | "file" | "expect";
	path?: string;
	fence: string;
}

function serializeCell(cell: ScenarioCell): string {
	if (cell.kind === "markup") {
		return cell.value.trim();
	}
	const body = cell.value.length ? ensureTrailingNewline(cell.value) : "";
	const fence = pickFence(body);
	if (cell.kind === "rules") {
		return `${fence}toml cm:rules\n${body}${fence}`;
	}
	if (cell.kind === "expect") {
		return `${fence}cm:expect\n${body}${fence}`;
	}
	const tag = cell.fence ? `${cell.fence} ` : "";
	return `${fence}${tag}cm:file=${cell.path}\n${body}${fence}`;
}

function pickFence(body: string): string {
	let fence = "```";
	while (body.includes(fence)) {
		fence += "`";
	}
	return fence;
}

function blockCell(block: BlockInfo, value: string): ScenarioCell {
	if (block.cmType === "rules") {
		return { kind: "rules", value };
	}
	if (block.cmType === "expect") {
		return { kind: "expect", value };
	}
	return {
		kind: "file",
		path: block.path ?? "",
		fence: block.fence,
		value,
	};
}

function consumeFrontMatter(lines: string[], document: ScenarioDocument): number {
	if (lines[0]?.trim() !== "---") {
		return 0;
	}
	const close = lines.findIndex((line, index) => index > 0 && line.trim() === "---");
	if (close < 0) {
		return 0;
	}
	document.frontMatter = joinBlock(lines.slice(1, close));
	return close + 1;
}

function flushProse(prose: string[], cells: ScenarioCell[]): void {
	const value = prose.join("\n").trim();
	if (value.length) {
		cells.push({ kind: "markup", value });
	}
}

function fenceLength(line: string): number {
	let length = 0;
	while (line[length] === "`") {
		length += 1;
	}
	return length >= 3 ? length : 0;
}

function findClosingFence(lines: string[], from: number, fence: number): number {
	for (let index = from; index < lines.length; index++) {
		const length = fenceLength(lines[index]);
		if (length >= fence && lines[index].trim().replace(/`/g, "") === "") {
			return index;
		}
	}
	return lines.length;
}

function classifyInfo(line: string): BlockInfo | undefined {
	const info = line.replace(/^`+/, "").trim();
	const tokens = info.split(/\s+/).filter(Boolean);
	const language = tokens[0]?.startsWith("cm:") ? "" : tokens[0] ?? "";
	for (const token of tokens) {
		if (token === "cm:rules") {
			return { cmType: "rules", fence: language };
		}
		if (token === "cm:expect") {
			return { cmType: "expect", fence: language };
		}
		if (token.startsWith("cm:file=")) {
			return {
				cmType: "file",
				path: token.slice("cm:file=".length),
				fence: language,
			};
		}
	}
	return undefined;
}

function joinBlock(lines: string[]): string {
	return lines.length ? lines.join("\n") + "\n" : "";
}

function ensureTrailingNewline(text: string): string {
	return text.endsWith("\n") || text.length === 0 ? text : `${text}\n`;
}
