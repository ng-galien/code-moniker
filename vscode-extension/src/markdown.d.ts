declare module "*.md" {
	const content: string;
	export default content;
}

declare module "code-moniker-sample-packs" {
	export const CATALOG_DOCUMENTS: readonly {
		category: "learn" | "sample";
		document: string;
	}[];
}
