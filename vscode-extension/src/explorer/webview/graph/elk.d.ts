// The worker-free ELK bundle ships without its own type declarations; it
// exposes the same constructor as the package root.
declare module "elkjs/lib/elk.bundled.js" {
	import ELK from "elkjs";

	export * from "elkjs";
	export default ELK;
}
