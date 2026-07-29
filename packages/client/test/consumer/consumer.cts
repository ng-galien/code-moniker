import client = require("@code-moniker/client");
import nodeClient = require("@code-moniker/client/node");

const version: number = client.PROTOCOL_VERSION;
const runtime = new nodeClient.NodeDaemonRuntime();
void version;
void runtime;
