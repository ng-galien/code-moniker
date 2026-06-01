import { useStore } from "./store";

export function readCount() {
	return useStore.getState().count;
}

export function replaceCount() {
	useStore.setState({ count: 1 });
}
