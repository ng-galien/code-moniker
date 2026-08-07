import { readFileSync } from "node:fs";

const client = JSON.parse(
	readFileSync(new URL("../package.json", import.meta.url), "utf8"),
);
const expected = {
	"darwin-arm64": ["@code-moniker/cli-darwin-arm64", "darwin", "arm64"],
	"darwin-x64": ["@code-moniker/cli-darwin-x64", "darwin", "x64"],
	"linux-x64": ["@code-moniker/cli-linux-x64", "linux", "x64"],
	"win32-x64": ["@code-moniker/cli-win32-x64", "win32", "x64"],
};

for (const [target, [name, os, cpu]] of Object.entries(expected)) {
	const manifest = JSON.parse(
		readFileSync(
			new URL(`../native-packages/${target}/package.json`, import.meta.url),
			"utf8",
		),
	);
	if (manifest.name !== name) {
		throw new Error(`${target} package name is ${manifest.name}, expected ${name}`);
	}
	if (manifest.version !== client.version) {
		throw new Error(`${name} version must match @code-moniker/client`);
	}
	if (client.optionalDependencies?.[name] !== client.version) {
		throw new Error(`${name} must be an exact-version optional dependency`);
	}
	if (manifest.os?.length !== 1 || manifest.os[0] !== os) {
		throw new Error(`${name} must target only ${os}`);
	}
	if (manifest.cpu?.length !== 1 || manifest.cpu[0] !== cpu) {
		throw new Error(`${name} must target only ${cpu}`);
	}
	if (target === "linux-x64" && manifest.libc !== undefined) {
		throw new Error(`${name} must remain installable on every Linux libc`);
	}
	if (!manifest.files?.includes("THIRD_PARTY_NOTICES")) {
		throw new Error(`${name} must publish the bundled binary's third-party notices`);
	}
}

console.log(`native package metadata verified: ${Object.keys(expected).length} targets`);
