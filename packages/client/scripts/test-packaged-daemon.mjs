import { execFileSync } from "node:child_process";
import {
	copyFileSync,
	mkdirSync,
	mkdtempSync,
	rmSync,
	writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const [target, binaryArgument] = process.argv.slice(2);
if (!target || !binaryArgument) {
	throw new Error(
		"usage: npm run test:daemon:packaged -- <platform-architecture> <code-moniker-binary>",
	);
}

const packageRoot = fileURLToPath(new URL("..", import.meta.url));
const binary = resolve(binaryArgument);
const temporaryRoot = mkdtempSync(
	join(tmpdir(), "code-moniker-packaged-runtime-"),
);
const nativeStage = join(temporaryRoot, "native-package");
const packDirectory = join(temporaryRoot, "packs");
const consumer = join(temporaryRoot, "consumer");
const npmEnvironment = {
	...process.env,
	npm_config_cache: join(temporaryRoot, "npm-cache"),
};
const npmCli = process.env.npm_execpath;
if (!npmCli) {
	throw new Error(
		"the packaged daemon test must run through npm so npm_execpath is available",
	);
}

try {
	mkdirSync(packDirectory);
	mkdirSync(consumer);
	run(
		process.execPath,
		[
			join(packageRoot, "scripts", "stage-native-package.mjs"),
			target,
			binary,
			nativeStage,
		],
		packageRoot,
	);
	const nativePack = pack(nativeStage);
	const clientPack = pack(packageRoot);

	writeFileSync(
		join(consumer, "package.json"),
		JSON.stringify({ private: true, type: "module" }),
	);
	copyFileSync(
		join(packageRoot, "scripts", "smoke-packaged-daemon.mjs"),
		join(consumer, "smoke-packaged-daemon.mjs"),
	);
	runNpm(
		[
			"install",
			"--ignore-scripts",
			"--omit=optional",
			clientPack,
			nativePack,
		],
		consumer,
	);
	run(process.execPath, ["smoke-packaged-daemon.mjs"], consumer);
} finally {
	rmSync(temporaryRoot, { recursive: true, force: true });
}

function pack(directory) {
	const output = execFileSync(
		process.execPath,
		[
			npmCli,
			"pack",
			directory,
			"--pack-destination",
			packDirectory,
		],
		{
			cwd: packageRoot,
			encoding: "utf8",
			env: npmEnvironment,
		},
	);
	const filename = output.trim().split(/\r?\n/).at(-1);
	if (!filename) {
		throw new Error(`npm pack returned no archive for ${directory}`);
	}
	return join(packDirectory, filename);
}

function runNpm(args, cwd) {
	run(process.execPath, [npmCli, ...args], cwd);
}

function run(command, args, cwd) {
	execFileSync(command, args, {
		cwd,
		env: npmEnvironment,
		stdio: "inherit",
	});
}
