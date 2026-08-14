import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import {
	canonicalJson,
	parseGitNameStatus,
	renderDiffImpact,
	runGitDiffImpact,
	sourceLanguageForPath,
} from "../dist/node.js";

const artifact = {
	schemaVersion: 1,
	kind: "code-moniker.diff-impact",
	repository: "https://example.invalid/team/sample.git",
	project: "sample",
	ticket: "CM-42",
	revisions: {
		base: { requested: "main", resolved: "aaaaaaaaaaaaaaaa" },
		head: { requested: "feature", resolved: "bbbbbbbbbbbbbbbb" },
	},
	scope: "aaaaaaaaaaaaaaaa..bbbbbbbbbbbbbbbb",
	inventory: {
		files: [
			{
				status: "modified", oldPath: "src/service.rs", newPath: "src/service.rs",
				renameScore: null, language: "rs", category: "source", analyzed: true, omission: null,
			},
			{
				status: "modified", oldPath: "docs/config.json", newPath: "docs/config.json",
				renameScore: null, language: null, category: "documentation", analyzed: false, omission: "unsupported language",
			},
		],
		totals: { added: 0, modified: 2, deleted: 0, renamed: 0 },
	},
	semantic: {
		scope: "aaaaaaaaaaaaaaaa..bbbbbbbbbbbbbbbb",
		summary: { files: 1, analyzable_files: 1, symbol_changes: 1, ref_changes: 1, retargeted_refs: 1, residual_files: 0 },
		files: [{ old_path: "src/service.rs", new_path: "src/service.rs", disposition: "modified", analyzable: true, symbol_changes: 1, moved_symbols: 0, coverage_explained: true, old_residual: [], new_residual: [], test_artifact: false }],
		symbol_changes: [{
			kind: "added", confidence: "certain", body_changed: false, signature_changed: false,
			visibility_changed: false, header_changed: false, file_moved: false, old: null,
			new: {
				identity: "code+moniker://sample/srcset:diff-impact/lang:rs/dir:src/module:service/fn:assess_impact()",
				compact_identity: "rs:src/service.fn:assess_impact()", file: "src/service.rs", kind: "fn",
				name: "assess_impact()", visibility: "public", lines: [1, 3], test_artifact: false,
			},
		}],
		ref_changes: [{
			kind: "call-site-retargeted", file: "src/service.rs", ref_kind: "call",
			old_target: "code+moniker://sample/srcset:diff-impact/lang:rs/dir:src/module:service/fn:old()",
			new_target: "code+moniker://sample/srcset:diff-impact/lang:rs/dir:src/module:service/fn:assess_impact()",
			old_target_compact: "rs:src/service.fn:old()", new_target_compact: "rs:src/service.fn:assess_impact()",
			old_lines: [2, 2], new_lines: [2, 2],
		}], diagnostics: [],
	},
	tests: { basis: "analyzed-path-and-extractor-kind", files: [], symbolChanges: 0 },
	coverage: { corpus: "changed-files", changedFiles: 2, analyzedFiles: 1, skippedFiles: 1, relations: "changed-file-extraction" },
	limitations: ["Bounded corpus."],
};

test("canonical diff-impact JSON is stable regardless of object insertion order", () => {
	const first = canonicalJson({ "é": 4, z: 1, A: 0, a: { y: 2, x: 3 } });
	const second = canonicalJson({ a: { x: 3, y: 2 }, A: 0, z: 1, "é": 4 });
	assert.equal(first, second);
	assert.equal(first, '{\n  "A": 0,\n  "a": {\n    "x": 3,\n    "y": 2\n  },\n  "z": 1,\n  "é": 4\n}\n');
});

test("the Git adapter accepts only explicit statuses", () => {
	assert.equal(parseGitNameStatus("M\0src/a.rs\0")[0].status, "modified");
	assert.throws(() => parseGitNameStatus("T\0src/a.rs\0"), /unsupported git diff status/);
	assert.throws(() => parseGitNameStatus("U\0src/a.rs\0"), /unsupported git diff status/);
});

test("source language adaptation covers every canonical workspace extension", () => {
	const expected = {
		"a.ts": "ts", "a.tsx": "ts", "a.js": "ts", "a.jsx": "ts", "a.mjs": "ts", "a.cjs": "ts",
		"a.rs": "rs", "a.java": "java", "a.py": "python", "a.pyi": "python", "a.go": "go",
		"a.c": "c", "a.h": "c", "a.cs": "cs", "a.sql": "sql", "a.sql.in": "sql", "a.plpgsql": "sql",
	};
	for (const [path, language] of Object.entries(expected)) {
		assert.equal(sourceLanguageForPath(path), language, path);
	}
});

test("cross-language renames are kept in inventory but excluded from semantic analysis", async () => {
	const repository = await mkdtemp(join(tmpdir(), "code-moniker-cross-language-"));
	const git = (...args) => execFileSync("git", ["-C", repository, ...args], { encoding: "utf8" }).trim();
	try {
		git("init", "--quiet");
		git("config", "user.email", "code-moniker@example.test");
		git("config", "user.name", "Code Moniker");
		await writeFile(join(repository, "module.py"), "def unchanged():\n    return 1\n");
		git("add", "module.py");
		git("commit", "--quiet", "-m", "base");
		const base = git("rev-parse", "HEAD");
		git("mv", "module.py", "module.rs");
		git("commit", "--quiet", "-m", "rename");
		const head = git("rev-parse", "HEAD");
		let compared;
		let stopped = false;
		const runtime = {
			async launch() {
				return { entry: {}, process: { terminate() {}, isRunning() { return false; } } };
			},
			async connect() {
				return {
					supportsQuery: (query) => query === "diff-impact.compare",
					diffImpact: {
						async compare(options) {
							compared = options;
							return {
								scope: options.scope,
								summary: { files: 0, analyzable_files: 0, symbol_changes: 0, ref_changes: 0, retargeted_refs: 0, residual_files: 0 },
								files: [], symbol_changes: [], ref_changes: [], diagnostics: [],
							};
						},
					},
					close() {},
				};
			},
			async stopOwned() { stopped = true; },
		};
		const output = await runGitDiffImpact(
			{ repository, base, head, project: "sample" },
			() => runtime,
		);
		assert.equal(output.artifact.inventory.files.length, 1);
		assert.deepEqual(output.artifact.inventory.files[0], {
			status: "renamed",
			oldPath: "module.py",
			newPath: "module.rs",
			renameScore: 100,
			language: "rs",
			category: "source",
			analyzed: false,
			omission: "language changed across rename (python -> rs)",
		});
		assert.equal(compared.files.length, 0);
		assert.equal(compared.base.documents.length, 0);
		assert.equal(compared.head.documents.length, 0);
		assert.equal(stopped, true);
	} finally {
		await rm(repository, { recursive: true, force: true });
	}
});

test("the diff-impact text is a compact projection of the canonical artifact", () => {
	const text = renderDiffImpact(artifact);
	assert.match(text, /2 changed files; 1 analyzed; 1 symbol changes/);
	assert.match(text, /- src\n  - src\/service\.rs — status=modified; analyzed; symbols=1; public=1; relations=1; tests=0/);
	assert.match(text, /    - \[added\] fn:assess_impact\(\)/);
	assert.doesNotMatch(text, /    - \[added\].*src\/service/);
	assert.match(text, /added \(1\): rs:src\/service\.fn:assess_impact\(\)/);
	assert.match(text, /rs:src\/service\.fn:old\(\) → rs:src\/service\.fn:assess_impact\(\)/);
	assert.doesNotMatch(text, /code\+moniker:\/\//);
	assert.match(text, /- docs\n  - docs\/config\.json — status=modified; outside-index; category=documentation; reason=unsupported language/);
	assert.match(text, /no changed analyzed test file/);
	assert.match(text, /Bounded corpus/);
});
