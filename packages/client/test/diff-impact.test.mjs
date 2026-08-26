import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { chmod, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { isAbsolute, join, resolve } from "node:path";
import test from "node:test";

import {
	canonicalJson,
	parseGitNameStatus,
	renderDiffImpact,
	runGitDiffImpact,
	sourceLanguageForPath,
} from "../dist/node.js";

const artifact = {
	schemaVersion: 2,
	kind: "code-moniker.diff-impact",
	repository: "https://example.invalid/team/sample.git",
	project: "sample",
	ticket: "CM-42",
	revisions: {
		base: { requested: "main", resolved: "aaaaaaaaaaaaaaaa" },
		head: { requested: "feature", resolved: "bbbbbbbbbbbbbbbb" },
	},
	scope: "aaaaaaaaaaaaaaaa..bbbbbbbbbbbbbbbb",
	runtimeDependencies: {
		git: {
			state: "available",
			processScope: "client",
			resolutionSource: "inherited_path",
			executable: "/usr/bin/git",
			version: "git version 2.50.1",
			supportedRange: ">=2.22.0",
			compatible: true,
			failure: null,
			checkedAt: "2026-08-26T00:00:00.000Z",
			durationMs: 4,
			repositoryState: "worktree",
		},
	},
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
			resolveBinaryCandidates() {
				return [resolve(process.cwd(), "../../target/release/code-moniker.exe")];
			},
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
		assert.equal(output.artifact.schemaVersion, 2);
		assert.equal(output.artifact.runtimeDependencies.git.state, "available");
		assert.equal(output.artifact.runtimeDependencies.git.processScope, "client");
		assert.match(output.artifact.runtimeDependencies.git.version, /^git version /);
		assert.equal(output.artifact.runtimeDependencies.git.failure, null);
		assert.equal(output.artifact.runtimeDependencies.git.repositoryState, "worktree");
		assert.equal(isAbsolute(output.artifact.runtimeDependencies.git.executable), true);
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

test("a blob command failure does not mark the Git runtime unavailable", async (context) => {
	if (process.platform === "win32") {
		context.skip("the portable shell wrapper is covered on Unix");
		return;
	}
	const directory = await mkdtemp(join(tmpdir(), "code-moniker-blob-failure-"));
	const repository = join(directory, "repository");
	const fakeGit = join(directory, "fake git");
	const git = (...args) => execFileSync("git", ["-C", repository, ...args], { encoding: "utf8" }).trim();
	try {
		execFileSync("git", ["init", "--quiet", repository]);
		git("config", "user.email", "code-moniker@example.test");
		git("config", "user.name", "Code Moniker");
		await writeFile(join(repository, "module.rs"), "pub fn value() -> i32 { 1 }\n");
		git("add", "module.rs");
		git("commit", "--quiet", "-m", "base");
		const base = git("rev-parse", "HEAD");
		await writeFile(join(repository, "module.rs"), "pub fn value() -> i32 { 2 }\n");
		git("commit", "--quiet", "-am", "head");
		const head = git("rev-parse", "HEAD");
		await writeFile(
			fakeGit,
			"#!/bin/sh\nfor arg in \"$@\"; do if [ \"$arg\" = show ]; then echo 'fatal: simulated missing blob' >&2; exit 128; fi; done\nexec git \"$@\"\n",
		);
		await chmod(fakeGit, 0o755);
		const runtime = {
			async launch() {
				return { entry: {}, process: { terminate() {}, isRunning() { return false; } } };
			},
			async connect() {
				return {
					supportsQuery: (query) => query === "diff-impact.compare",
					diffImpact: {
						async compare(options) {
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
			async stopOwned() {},
		};

		const output = await runGitDiffImpact(
			{ repository, base, head, project: "sample", gitBinary: fakeGit },
			() => runtime,
		);
		assert.equal(output.artifact.inventory.files[0].analyzed, false);
		assert.match(output.artifact.inventory.files[0].omission, /simulated missing blob/);
		assert.equal(output.artifact.runtimeDependencies.git.state, "available");
		assert.equal(output.artifact.runtimeDependencies.git.failure, null);
	} finally {
		await rm(directory, { recursive: true, force: true });
	}
});

test("the Git diagnostic rejects a non-absolute explicit executable without PATH fallback", async () => {
	const runtime = {
		async launch() { throw new Error("runtime must not launch"); },
	};
	await assert.rejects(
		runGitDiffImpact(
			{ repository: ".", base: "HEAD", head: "HEAD", gitBinary: "git" },
			() => runtime,
		),
		(error) => {
			assert.equal(error.name, "GitRuntimeError");
			assert.equal(error.state, "unavailable");
			assert.equal(error.diagnostic.resolutionSource, "explicit_configuration");
			assert.equal(error.diagnostic.failure.category, "invalid_configuration");
			assert.match(error.message, /absolute executable path/);
			return true;
		},
	);
});

test("the Git diagnostic rejects an empty explicit executable without PATH fallback", async () => {
	await assert.rejects(
		runGitDiffImpact(
			{ repository: ".", base: "HEAD", head: "HEAD", gitBinary: "" },
			() => ({ async launch() { throw new Error("runtime must not launch"); } }),
		),
		(error) => {
			assert.equal(error.diagnostic.resolutionSource, "explicit_configuration");
			assert.equal(error.diagnostic.failure.category, "invalid_configuration");
			assert.match(error.message, /must not be empty/);
			return true;
		},
	);
});

test("the Git diagnostic ignores empty inherited PATH segments", async () => {
	await assert.rejects(
		runGitDiffImpact(
			{
				repository: ".",
				base: "HEAD",
				head: "HEAD",
				environment: {
					PATH: `${join(tmpdir(), "no-git-here")}${process.platform === "win32" ? ";" : ":"}`,
					CODE_MONIKER_GIT_BINARY: undefined,
				},
			},
			() => ({ async launch() { throw new Error("runtime must not launch"); } }),
		),
		(error) => {
			assert.equal(error.diagnostic.failure.category, "not_found");
			return true;
		},
	);
});

test("the Git diagnostic distinguishes a non-executable explicit file", async (context) => {
	if (process.platform === "win32") {
		context.skip("Windows executable permission is validated by native process creation");
		return;
	}
	const directory = await mkdtemp(join(tmpdir(), "code-moniker-denied-git-"));
	const fakeGit = join(directory, "fake git");
	try {
		await writeFile(fakeGit, "#!/bin/sh\necho 'git version 2.47.1'\n");
		await chmod(fakeGit, 0o644);
		await assert.rejects(
			runGitDiffImpact(
				{ repository: ".", base: "HEAD", head: "HEAD", gitBinary: fakeGit },
				() => ({ async launch() { throw new Error("runtime must not launch"); } }),
			),
			(error) => {
				assert.equal(error.name, "GitRuntimeError");
				assert.equal(error.diagnostic.failure.category, "permission_denied");
				assert.equal(error.diagnostic.executable, fakeGit);
				return true;
			},
		);
	} finally {
		await rm(directory, { recursive: true, force: true });
	}
});

test("the Git diagnostic aligns malformed and incompatible version taxonomy", async (context) => {
	if (process.platform === "win32") {
		context.skip("native fake-executable coverage runs in the Windows CI job");
		return;
	}
	const directory = await mkdtemp(join(tmpdir(), "code-moniker-fake-git-"));
	const fakeGit = join(directory, "fake git");
	try {
		await writeFile(fakeGit, "#!/bin/sh\nif [ \"$CODE_MONIKER_FAKE_GIT_MODE\" = malformed ]; then echo 'not git'; elif [ \"$CODE_MONIKER_FAKE_GIT_MODE\" = malformed_suffix ]; then echo 'git version 2.22.0evil'; else echo 'git version 2.21.0'; fi\n");
		await chmod(fakeGit, 0o755);
		for (const [mode, state, category] of [
			["malformed", "unavailable", "malformed_version"],
			["malformed_suffix", "unavailable", "malformed_version"],
			["incompatible", "incompatible", "incompatible_version"],
		]) {
			await assert.rejects(
				runGitDiffImpact(
					{
						repository: ".",
						base: "HEAD",
						head: "HEAD",
						gitBinary: fakeGit,
						environment: { CODE_MONIKER_FAKE_GIT_MODE: mode },
					},
					() => ({ async launch() { throw new Error("runtime must not launch"); } }),
				),
				(error) => {
					assert.equal(error.state, state);
					assert.equal(error.diagnostic.processScope, "client");
					assert.equal(error.diagnostic.failure.category, category);
					return true;
				},
			);
		}
	} finally {
		await rm(directory, { recursive: true, force: true });
	}
});

test("a successful repository probe with malformed flags is unavailable", async (context) => {
	if (process.platform === "win32") {
		context.skip("the compiled fake Git runtime is exercised by the Windows CI job");
		return;
	}
	const directory = await mkdtemp(join(tmpdir(), "code-moniker-malformed-root-git-"));
	const fakeGit = join(directory, "fake git");
	try {
		await writeFile(fakeGit, "#!/bin/sh\nif [ \"$1\" = --version ]; then echo 'git version 2.47.1'; else echo 'false'; fi\n");
		await chmod(fakeGit, 0o755);
		await assert.rejects(
			runGitDiffImpact(
				{ repository: ".", base: "HEAD", head: "HEAD", gitBinary: fakeGit },
				() => ({ async launch() { throw new Error("runtime must not launch"); } }),
			),
			(error) => {
				assert.equal(error.state, "unavailable");
				assert.equal(error.diagnostic.failure.category, "malformed_output");
				return true;
			},
		);
	} finally {
		await rm(directory, { recursive: true, force: true });
	}
});

test("a non-repository probe has a consistent unavailable diagnostic", async (context) => {
	if (process.platform === "win32") {
		context.skip("the compiled fake Git runtime is exercised by the Windows CI job");
		return;
	}
	const directory = await mkdtemp(join(tmpdir(), "code-moniker-non-repository-git-"));
	const fakeGit = join(directory, "fake git");
	try {
		await writeFile(fakeGit, "#!/bin/sh\nif [ \"$1\" = --version ]; then echo 'git version 2.47.1'; else echo 'fatal: not a git repository' >&2; exit 128; fi\n");
		await chmod(fakeGit, 0o755);
		await assert.rejects(
			runGitDiffImpact(
				{ repository: directory, base: "HEAD", head: "HEAD", gitBinary: fakeGit },
				() => ({ async launch() { throw new Error("runtime must not launch"); } }),
			),
			(error) => {
				assert.equal(error.state, "unavailable");
				assert.equal(error.diagnostic.state, "unavailable");
				assert.equal(error.diagnostic.repositoryState, "not_repository");
				assert.equal(error.diagnostic.failure.category, "not_repository");
				return true;
			},
		);
	} finally {
		await rm(directory, { recursive: true, force: true });
	}
});

test("a repository probe timeout preserves the timed_out taxonomy", async (context) => {
	if (process.platform === "win32") {
		context.skip("the compiled fake Git runtime is exercised by the Windows CI job");
		return;
	}
	const directory = await mkdtemp(join(tmpdir(), "code-moniker-hanging-git-"));
	const fakeGit = join(directory, "fake git");
	const descendantPidFile = join(directory, "descendant.pid");
	try {
		await writeFile(fakeGit, "#!/bin/sh\nif [ \"$1\" = --version ]; then echo 'git version 2.47.1.windows.1'; else sleep 30 & echo $! > \"$CODE_MONIKER_FAKE_GIT_PID_FILE\"; wait; fi\n");
		await chmod(fakeGit, 0o755);
		await assert.rejects(
			runGitDiffImpact(
				{
					repository: ".",
					base: "HEAD",
					head: "HEAD",
					gitBinary: fakeGit,
					environment: { CODE_MONIKER_FAKE_GIT_PID_FILE: descendantPidFile },
				},
				() => ({ async launch() { throw new Error("runtime must not launch"); } }),
			),
			(error) => {
				assert.equal(error.name, "GitRuntimeError");
				assert.equal(error.state, "timed_out");
				assert.equal(error.diagnostic.failure.category, "timed_out");
				return true;
			},
		);
		const descendantPid = Number.parseInt((await readFile(descendantPidFile, "utf8")).trim(), 10);
		await waitForProcessExit(descendantPid);
	} finally {
		await rm(directory, { recursive: true, force: true });
	}
});

test("revision verification uses the two-second metadata budget", async (context) => {
	if (process.platform === "win32") {
		context.skip("the compiled fake Git runtime is exercised by the Windows CI job");
		return;
	}
	const directory = await mkdtemp(join(tmpdir(), "code-moniker-slow-revision-git-"));
	const fakeGit = join(directory, "fake git");
	try {
		await writeFile(fakeGit, `#!/bin/sh
case " $* " in
  *" --version "*) echo 'git version 2.47.1' ;;
  *" --is-inside-work-tree "*) printf 'true\\nfalse\\n' ;;
  *" --verify "*) sleep 30 ;;
  *) exit 1 ;;
esac
`);
		await chmod(fakeGit, 0o755);
		const started = performance.now();
		await assert.rejects(
			runGitDiffImpact(
				{
					repository: ".",
					base: "HEAD",
					head: "HEAD",
					gitBinary: fakeGit,
				},
				() => ({ async launch() { throw new Error("runtime must not launch"); } }),
			),
			(error) => error?.diagnostic?.failure?.category === "timed_out",
		);
		assert.ok(performance.now() - started < 4_000);
	} finally {
		await rm(directory, { recursive: true, force: true });
	}
});

async function waitForProcessExit(pid) {
	const deadline = Date.now() + 1_000;
	while (Date.now() <= deadline) {
		try {
			process.kill(pid, 0);
		} catch (error) {
			if (error?.code === "ESRCH") return;
			throw error;
		}
		await new Promise((resolve) => setTimeout(resolve, 20));
	}
	assert.fail(`descendant process ${pid} was not reaped after the Git timeout`);
}

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
