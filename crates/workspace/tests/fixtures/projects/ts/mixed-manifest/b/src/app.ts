import { useStore } from "./store";

export function fromB() {
	return useStore.getState().count;
}
