import {
	chmodSync,
	copyFileSync,
	existsSync,
	mkdirSync,
	readFileSync,
	writeFileSync,
} from "node:fs";
import { basename, join, resolve } from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const targets = {
	"darwin-arm64": "code-moniker",
	"darwin-x64": "code-moniker",
	"linux-x64": "code-moniker",
	"win32-x64": "code-moniker.exe",
};

const [target, binaryArgument, outputArgument] = process.argv.slice(2);
if (!target || !binaryArgument || !outputArgument || !(target in targets)) {
	throw new Error(
		"usage: node scripts/stage-native-package.mjs <darwin-arm64|darwin-x64|linux-x64|win32-x64> <binary> <empty-output-directory>",
	);
}

const packageRoot = fileURLToPath(new URL("..", import.meta.url));
const binary = resolve(binaryArgument);
const output = resolve(outputArgument);
if (!existsSync(binary)) {
	throw new Error(`native binary does not exist: ${binary}`);
}
if (existsSync(output)) {
	throw new Error(`native package output already exists: ${output}`);
}

const manifestPath = join(
	packageRoot,
	"native-packages",
	target,
	"package.json",
);
const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
const executable = targets[target];
mkdirSync(join(output, "bin"), { recursive: true });
copyFileSync(binary, join(output, "bin", executable));
if (target !== "win32-x64") {
	chmodSync(join(output, "bin", executable), 0o755);
}
copyFileSync(join(packageRoot, "LICENSE"), join(output, "LICENSE"));
writeFileSync(
	join(output, "README.md"),
	`# \`${manifest.name}\`\n\nPrecompiled ${basename(executable)} binary used internally by \`@code-moniker/client/node\`.\n`,
);
copyFileSync(manifestPath, join(output, "package.json"));

console.log(`${manifest.name}@${manifest.version} staged in ${output}`);
