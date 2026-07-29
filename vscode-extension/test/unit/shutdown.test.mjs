import assert from "node:assert/strict";
import { resolve } from "node:path";
import test from "node:test";

import { build } from "esbuild";

const source = resolve("src/daemon/shutdown.ts");
const output = await build({
	entryPoints: [source],
	bundle: true,
	format: "esm",
	platform: "node",
	write: false,
});
const moduleUrl = `data:text/javascript;base64,${Buffer.from(
	output.outputFiles[0].text,
).toString("base64")}`;
const { withShutdownCleanup } = await import(moduleUrl);

test("shutdown cleanup always runs and preserves the operation failure", async () => {
	const expected = new Error("daemon did not exit");
	let cleaned = false;

	await assert.rejects(
		withShutdownCleanup(
			async () => {
				throw expected;
			},
			() => {
				cleaned = true;
			},
		),
		(error) => error === expected,
	);
	assert.equal(cleaned, true);
});

test("shutdown cleanup also runs after a successful stop", async () => {
	const calls = [];
	await withShutdownCleanup(
		async () => {
			calls.push("stop");
		},
		() => {
			calls.push("cleanup");
		},
	);
	assert.deepEqual(calls, ["stop", "cleanup"]);
});
