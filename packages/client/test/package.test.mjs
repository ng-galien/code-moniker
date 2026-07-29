import assert from "node:assert/strict";
import { createRequire } from "node:module";
import test from "node:test";

import * as esmClient from "../dist/index.js";
import * as esmNodeClient from "../dist/node.js";

const require = createRequire(import.meta.url);
const commonJsClient = require("../dist/index.cjs");
const commonJsNodeClient = require("../dist/node.cjs");

test("the package exposes the same public entry point to ESM and CommonJS consumers", () => {
	for (const client of [esmClient, commonJsClient]) {
		assert.equal(typeof client.CodeMonikerClient.connect, "function");
		assert.equal(typeof client.PROTOCOL_VERSION, "number");
		assert.equal(typeof client.ProtocolMismatchError, "function");
		assert.equal("DaemonRpc" in client, false);
	}
});

test("the Node entry point exposes lifecycle APIs to ESM and CommonJS consumers", () => {
	for (const client of [esmNodeClient, commonJsNodeClient]) {
		assert.equal(typeof client.NodeDaemonRuntime, "function");
		assert.equal(typeof client.nodeWebSocketFactory, "function");
	}
});
