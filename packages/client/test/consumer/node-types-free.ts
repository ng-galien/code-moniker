import {
	NodeDaemonRuntime,
	type LaunchDaemonOptions,
} from "@code-moniker/client/node";

const options: LaunchDaemonOptions = {
	workspaceRoots: ["/workspace/project"],
	binaryCandidates: ["code-moniker"],
	environment: {
		CODE_MONIKER_LOG: "info",
		OPTIONAL_VALUE: undefined,
	},
};

void new NodeDaemonRuntime();
void options;
