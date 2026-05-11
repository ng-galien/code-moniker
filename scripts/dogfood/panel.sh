#!/usr/bin/env bash
# Panel of representative open-source projects per extractor language.
#
# Entry format (fields separated by `|`):
#   lang | project_id | git_url | ref | src_subdir | manifest_path
# - lang          rs | ts | java | sql | py | go | cs
# - project_id    moniker project segment + DB key; stable across runs
# - git_url       HTTPS clone URL, or `self` for the local repo
# - ref           pinned tag/branch/commit; `HEAD` only with `self`
# - src_subdir    source-file root under the clone
# - manifest_path build manifest path, or `-` to skip

PCM_DOGFOOD_PANEL=(
	# Rust
	"rs|code-moniker|self|HEAD|src|Cargo.toml"
	"rs|clap|https://github.com/clap-rs/clap.git|v4.5.20|clap_builder/src|clap_builder/Cargo.toml"
	"rs|bytes|https://github.com/tokio-rs/bytes.git|v1.7.2|src|Cargo.toml"

	# TypeScript
	"ts|zod|https://github.com/colinhacks/zod.git|v3.23.8|src|package.json"
	"ts|date-fns|https://github.com/date-fns/date-fns.git|v3.6.0|src|package.json"

	# Java
	"java|gson|https://github.com/google/gson.git|gson-parent-2.11.0|gson/src/main/java|-"

	# SQL / PL-pgSQL
	"sql|pgtap|https://github.com/theory/pgtap.git|v1.3.3|sql|-"

	# Python
	"py|httpx|https://github.com/encode/httpx.git|0.27.2|httpx|-"

	# Go
	"go|mux|https://github.com/gorilla/mux.git|v1.8.1|.|go.mod"

	# C#
	"cs|commandline|https://github.com/commandlineparser/commandline.git|v2.9.1|src/CommandLine|src/CommandLine/CommandLine.csproj"
)
