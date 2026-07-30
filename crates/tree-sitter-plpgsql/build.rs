fn main() {
	let source_dir = std::path::Path::new("src");
	let parser = source_dir.join("parser.c");
	let scanner = source_dir.join("scanner.c");

	cc::Build::new()
		.std("c11")
		.include(source_dir)
		.file(&parser)
		.file(&scanner)
		.compile("code-moniker-tree-sitter-plpgsql");

	println!("cargo:rerun-if-changed={}", parser.display());
	println!("cargo:rerun-if-changed={}", scanner.display());
}
