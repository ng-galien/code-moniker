import { DaemonRegistryEntry } from "./model";

export interface DaemonNode {
	entry: DaemonRegistryEntry;
	current: boolean;
}
