import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const packageManifest = JSON.parse(
	readFileSync(new URL("../package.json", import.meta.url), "utf8"),
);
for (const [entryPoint, conditions] of Object.entries(
	packageManifest.exports,
)) {
	if ("source" in conditions) {
		throw new Error(
			`published export ${entryPoint} must not expose raw TypeScript through a source condition`,
		);
	}
}

const cache = mkdtempSync(join(tmpdir(), "code-moniker-client-pack-"));
let output;
try {
	output = execFileSync("npm", ["pack", "--dry-run", "--json"], {
		encoding: "utf8",
		env: {
			...process.env,
			npm_config_cache: cache,
		},
	});
} finally {
	rmSync(cache, { recursive: true, force: true });
}
const [manifest] = JSON.parse(output);
const files = new Set(manifest.files.map(filePath));
for (const file of files) {
	if (file.startsWith("src/")) {
		throw new Error(`npm package must not publish source file ${file}`);
	}
}

for (const required of [
	"LICENSE",
	"README.md",
	"dist/index.js",
	"dist/index.cjs",
	"dist/index.d.ts",
	"dist/node.js",
	"dist/node.cjs",
	"dist/node.d.ts",
	"package.json",
]) {
	if (!files.has(required)) {
		throw new Error(`npm package is missing ${required}`);
	}
}

console.log(
	`npm package verified: ${manifest.files.length} files, ${manifest.size} bytes`,
);

function filePath(file) {
	return file.path;
}
