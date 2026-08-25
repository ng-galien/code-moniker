import { execFileSync } from "node:child_process";
import {
	copyFileSync,
	existsSync,
	mkdirSync,
	mkdtempSync,
	readFileSync,
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
	CODE_MONIKER_EXPECTED_BINARY_FINGERPRINT: binaryFingerprint(binary),
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
	const nativeManifest = JSON.parse(
		readFileSync(join(nativeStage, "package.json"), "utf8"),
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
	copyFileSync(
		join(packageRoot, "scripts", "seed-daemon-workspace.mjs"),
		join(consumer, "seed-daemon-workspace.mjs"),
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
	const installedNotice = join(
		consumer,
		"node_modules",
		...nativeManifest.name.split("/"),
		"THIRD_PARTY_NOTICES",
	);
	if (!existsSync(installedNotice)) {
		throw new Error(`${nativeManifest.name} is missing THIRD_PARTY_NOTICES`);
	}
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

function binaryFingerprint(path) {
	let hash = 0xcbf29ce484222325n;
	for (const byte of readFileSync(path)) {
		hash ^= BigInt(byte);
		hash = BigInt.asUintN(64, hash * 0x100000001b3n);
	}
	return `fnv1a64:${hash.toString(16).padStart(16, "0")}`;
}
