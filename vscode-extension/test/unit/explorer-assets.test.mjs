import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const extensionRoot = resolve(dirname(fileURLToPath(import.meta.url)), "../..");

test("the cockpit ships React Flow's structural stylesheet", async () => {
	const source = await readFile(
		resolve(extensionRoot, "src/explorer/webview/explorer.css"),
		"utf8",
	);
	assert.match(source, /^@import "@xyflow\/react\/dist\/style\.css";/);

	const bundled = await readFile(resolve(extensionRoot, "media/explorer/explorer.css"), "utf8");
	assert.match(bundled, /\.react-flow__viewport/);
	assert.match(bundled, /\.react-flow__pane/);
	assert.match(bundled, /touch-action:none/);
});

test("the cockpit representation exposes overview, predictable navigation and explicit refocus", async () => {
	const canvas = await readFile(
		resolve(extensionRoot, "src/explorer/webview/CockpitCanvas.tsx"),
		"utf8",
	);
	const card = await readFile(
		resolve(extensionRoot, "src/explorer/webview/graph/CockpitCard.tsx"),
		"utf8",
	);
	const edge = await readFile(
		resolve(extensionRoot, "src/explorer/webview/graph/CockpitEdge.tsx"),
		"utf8",
	);

	assert.match(canvas, /<MiniMap[\s\S]*pannable[\s\S]*zoomable/);
	assert.match(canvas, /panOnScroll=\{false\}/);
	assert.match(canvas, /Drag canvas to pan · scroll to zoom/);
	assert.match(canvas, /cockpit-edge-inspector/);
	assert.match(card, /onClick=\{\(\) => data\.onInspect\(symbol\.uri\)\}/);
	assert.match(card, />\s*Refocus\s*</);
	assert.match(edge, /getBezierPath/);
});
