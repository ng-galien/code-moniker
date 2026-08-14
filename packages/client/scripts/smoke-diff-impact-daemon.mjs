import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

import { NodeDaemonRuntime, runGitDiffImpact } from "../dist/node.js";

const [binaryArgument] = process.argv.slice(2);
if (!binaryArgument) {
	throw new Error("usage: npm run test:diff-impact:daemon -- <code-moniker-binary>");
}

const binary = resolve(binaryArgument);
const fixture = mkdtempSync(join(tmpdir(), "code-moniker-diff-impact-smoke-"));
const repository = join(fixture, "repository");
mkdirSync(repository);
let runtime;
let owned;
let failNextStop = false;

class ObservedRuntime extends NodeDaemonRuntime {
	async launch(options) {
		owned = await super.launch(options);
		return owned;
	}

	async stopOwned(claim, options) {
		if (failNextStop) {
			failNextStop = false;
			throw new Error("injected graceful shutdown failure");
		}
		return super.stopOwned(claim, options);
	}
}

try {
	git(["init", repository]);
	git(["-C", repository, "config", "user.name", "Code Moniker Test"]);
	git(["-C", repository, "config", "user.email", "test@code-moniker.invalid"]);
	mkdirSync(join(repository, "src"));
	writeFileSync(join(repository, "src", "service.rs"), "pub fn service() { old_dependency(); }\n");
	git(["-C", repository, "add", "src/service.rs"]);
	git(["-C", repository, "commit", "-m", "base"]);
	const base = git(["-C", repository, "rev-parse", "HEAD"]).trim();

	writeFileSync(join(repository, "src", "service.rs"), "pub fn service() { new_dependency(); }\n");
	mkdirSync(join(repository, "tests"));
	writeFileSync(join(repository, "tests", "service_test.rs"), "#[test]\nfn service_works() { service(); }\n");
	git(["-C", repository, "add", "src/service.rs", "tests/service_test.rs"]);
	git(["-C", repository, "commit", "-m", "head"]);
	const head = git(["-C", repository, "rev-parse", "HEAD"]).trim();

	const output = await runGitDiffImpact(
		{ repository, base, head, binaryCandidates: [binary], ticket: "CM-REVIEW" },
		(registryDirectory) => {
			runtime = new ObservedRuntime({ registryDirectory });
			return runtime;
		},
	);
	assert.equal(output.artifact.revisions.base.resolved, base);
	assert.equal(output.artifact.revisions.head.resolved, head);
	assert.equal(output.artifact.coverage.changedFiles, 2);
	assert.equal(output.artifact.coverage.analyzedFiles, 2);
	assert.ok(output.artifact.semantic.summary.symbol_changes >= 2);
	assert.deepEqual(JSON.parse(output.json), output.artifact);
	assert.match(output.text, /Tests: 1 changed analyzed test files/);
	assert.equal(owned?.process.isRunning(), false, "owned diff-impact daemon must be stopped");
	assert.deepEqual(runtime?.listDaemons(), [], "diff-impact registry must retain no claim");

	failNextStop = true;
	const remoteOutput = await runGitDiffImpact(
		{ repository: `file://${repository}`, base, head, binaryCandidates: [binary] },
		(registryDirectory) => {
			runtime = new ObservedRuntime({ registryDirectory });
			return runtime;
		},
	);
	assert.equal(remoteOutput.artifact.coverage.changedFiles, 2);
	assert.equal(owned?.process.isRunning(), false, "fallback must stop the remote diff-impact daemon");
	assert.deepEqual(runtime?.listDaemons(), [], "remote diff-impact registry must retain no claim");
	console.log(`diff impact smoke passed: ${base.slice(0, 12)}..${head.slice(0, 12)}`);
} finally {
	owned?.process.terminate();
	rmSync(fixture, { recursive: true, force: true });
}

function git(args) {
	return execFileSync("git", args, { encoding: "utf8" });
}
