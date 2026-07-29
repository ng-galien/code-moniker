import { execFileSync } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

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

for (const required of [
	"LICENSE",
	"README.md",
	"dist/index.js",
	"dist/index.cjs",
	"dist/index.d.ts",
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
