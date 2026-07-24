import { useStore } from "./store";

export function readCount(): number {
	return useStore.getState().count;
}
