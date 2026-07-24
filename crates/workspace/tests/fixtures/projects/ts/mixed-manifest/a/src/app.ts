import { useStore } from "./store";

export function fromA() {
	return useStore.getState().count;
}
