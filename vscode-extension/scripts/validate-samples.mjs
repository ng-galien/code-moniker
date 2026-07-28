import { readFileSync, readdirSync } from "node:fs";
import { basename, dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { build } from "esbuild";

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(here, "..", "..");
const catalogDir = join(repoRoot, "samples", "catalog");
const learnDir = join(repoRoot, "samples", "learn");
const packsSource = join(here, "..", "src", "catalog", "packs.ts");
const rulesParserSource = join(here, "..", "src", "rules", "parse.ts");

const catalog = readdirSync(catalogDir)
	.filter((name) => name.endsWith(".cm.md"))
	.sort();
const learn = readdirSync(learnDir)
	.filter((name) => name.endsWith(".cm.md"))
	.sort();
const source = readFileSync(packsSource, "utf8");

if (!source.includes('from "code-moniker-sample-packs"')) {
	console.error("Catalog packs must import the generated sample pack module.");
	process.exit(1);
}

if (/samples\/(catalog|learn)\//.test(source)) {
	console.error("Catalog packs must not manually import individual sample files.");
	process.exit(1);
}

for (const [dir, names] of [[catalogDir, catalog], [learnDir, learn]]) {
	for (const name of names) {
		const document = readFileSync(join(dir, name), "utf8");
		validateExecutableScenario(name, document);
	}
}

const parserBuild = await build({
	entryPoints: [rulesParserSource],
	bundle: true,
	platform: "node",
	format: "esm",
	write: false,
	logLevel: "silent",
});
const parserModule = await import(
	`data:text/javascript;base64,${Buffer.from(parserBuild.outputFiles[0].text).toString("base64")}`
);
const workspacePathRules = parserModule.parseRuleFile(`
[[workspace.path]]
id = "catalog-path-contract"
from = "shape = 'callable'"
to = "shape = 'type'"
expect = "reachable"
`).rules;
if (!workspacePathRules.some((rule) => rule.scope === "workspace.path")) {
	console.error("Catalog rule parsing must expose [[workspace.path]] entries.");
	process.exit(1);
}

const workspacePathDocument = readFileSync(join(catalogDir, "workspace-path.cm.md"), "utf8");
const workspacePathBlock = /^```toml cm:rules\n([\s\S]*?)^```$/m.exec(workspacePathDocument)?.[1];
const catalogWorkspacePathRules = workspacePathBlock
	? parserModule.parseRuleFile(workspacePathBlock).rules.filter(
		(rule) => rule.scope === "workspace.path",
	)
	: [];
if (catalogWorkspacePathRules.length < 3) {
	console.error("The workspace-path catalog entry must expose its path rules in the Catalog tree.");
	process.exit(1);
}

function validateExecutableScenario(name, document) {
	for (const token of ["cm:rules", "cm:file=", "cm:expect"]) {
		if (!document.includes(token)) {
			console.error(`${basename(name)} is missing ${token}`);
			process.exit(1);
		}
	}
}

console.log(
	`All ${catalog.length} catalog samples and ${learn.length} learn samples are imported by the extension.`,
);
