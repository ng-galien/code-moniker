export class ExplorerNavigation {
	private entries: string[] = [];
	private index = -1;

	get current(): string | undefined {
		return this.entries[this.index];
	}

	get canBack(): boolean {
		return this.index > 0;
	}

	get canForward(): boolean {
		return this.index >= 0 && this.index < this.entries.length - 1;
	}

	clear(): void {
		this.entries = [];
		this.index = -1;
	}

	push(focus: string): void {
		if (this.current === focus) return;
		this.entries.splice(this.index + 1);
		this.entries.push(focus);
		this.index = this.entries.length - 1;
	}

	replace(focus: string): void {
		if (this.index >= 0) this.entries[this.index] = focus;
	}

	move(delta: -1 | 1): string | undefined {
		const next = this.index + delta;
		if (next < 0 || next >= this.entries.length) return undefined;
		this.index = next;
		return this.current;
	}
}
