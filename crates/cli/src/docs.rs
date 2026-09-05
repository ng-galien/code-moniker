use std::io::Write;

use serde::Serialize;

use crate::Exit;
use crate::args::DocsArgs;

#[derive(Serialize)]
struct Document {
	path: &'static str,
	body: &'static str,
}

macro_rules! documents {
	($($path:literal),+ $(,)?) => {
		&[$(Document {
			path: $path,
			body: include_str!(concat!("../assets/docs/", $path)),
		}),+]
	};
}

const DOCUMENTS: &[Document] = documents![
	"README.md",
	"check-scenarios.md",
	"cli/agent.md",
	"cli/check.md",
	"cli/check-dsl.md",
	"cli/check-samples/README.md",
	"cli/code-smell-review.md",
	"cli/diff.md",
	"cli/extract.md",
	"cli/langs.md",
	"cli/manifest.md",
	"cli/mcp.md",
	"cli/mcp-syntax-tree.md",
	"cli/query.md",
	"cli/stats.md",
	"daemon.md",
	"design/agent-output-boundary.md",
	"design/documentation-system.md",
	"design/git-runtime-dependency.md",
	"design/moniker-uri.md",
	"observability.md",
	"perf.md",
	"release.md",
	"schema/daemon.schema.json",
	"source-groups.md",
	"vscode-extension.md",
];

#[derive(Serialize)]
struct DocumentSummary {
	path: &'static str,
	title: &'static str,
}

pub(crate) fn run<W: Write, E: Write>(args: &DocsArgs, stdout: &mut W, stderr: &mut E) -> Exit {
	match render(args, stdout) {
		Ok(()) => Exit::Match,
		Err(error) => {
			let _ = writeln!(stderr, "code-moniker: {error:#}");
			Exit::UsageError
		}
	}
}

fn render(args: &DocsArgs, stdout: &mut impl Write) -> anyhow::Result<()> {
	if let Some(path) = &args.page {
		let document = DOCUMENTS
			.iter()
			.find(|document| document.path == path)
			.ok_or_else(|| {
				anyhow::anyhow!(
					"unknown documentation page `{path}`; run `code-moniker docs` to list bundled pages"
				)
			})?;
		if args.json {
			serde_json::to_writer_pretty(&mut *stdout, document)?;
			writeln!(stdout)?;
		} else {
			stdout.write_all(document.body.as_bytes())?;
		}
		return Ok(());
	}
	let pages: Vec<_> = DOCUMENTS
		.iter()
		.map(|document| DocumentSummary {
			path: document.path,
			title: document
				.body
				.lines()
				.find_map(|line| line.strip_prefix("# "))
				.unwrap_or(document.path),
		})
		.collect();
	if args.json {
		serde_json::to_writer_pretty(&mut *stdout, &pages)?;
		writeln!(stdout)?;
	} else {
		writeln!(
			stdout,
			"# Bundled documentation\n\nRead a page with `code-moniker docs <path>`; use `--json` for structured output.\nPaths below are relative to docs/. Content matches this binary's version.\n\nStart with `code-moniker docs README.md` for the task map.\nRule tutorials remain available through `code-moniker rules learn`.\n\n| Path | Title |\n| --- | --- |"
		)?;
		for page in pages {
			writeln!(stdout, "| `{}` | {} |", page.path, page.title)?;
		}
	}
	Ok(())
}
