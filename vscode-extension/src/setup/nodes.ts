import { AgentClient, AgentComponent, AgentIntegration, CliState } from "./model";

// Rows of the Setup section. `cli` and `rules` answer "can this workspace be
// analysed at all"; `agent` answers "which assistant is wired to it". Nodes
// carry their DTO rather than copying its fields, like the sibling trees.
export interface SetupCliNode {
	kind: "cli";
	cli: CliState;
}

export interface SetupRulesNode {
	kind: "rules";
	present: boolean;
}

export interface SetupAgentNode {
	kind: "agent";
	integration: AgentIntegration;
}

export interface SetupComponentNode {
	kind: "component";
	client: AgentClient;
	component: AgentComponent;
}

export type SetupTreeNode = SetupCliNode | SetupRulesNode | SetupAgentNode | SetupComponentNode;
