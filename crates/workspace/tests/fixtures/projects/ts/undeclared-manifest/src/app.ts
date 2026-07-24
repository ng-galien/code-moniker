import { create } from "zustand";

export const useStore = create(() => ({ count: 0 }));

export function readCount(): number {
	return useStore.getState().count;
}
