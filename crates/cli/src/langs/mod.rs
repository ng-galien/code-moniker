use std::io::Write;

use crate::args::{LangsArgs, LangsFormat};
use crate::{Exit, language_kinds};

pub fn run<W1: Write, W2: Write>(args: &LangsArgs, stdout: &mut W1, stderr: &mut W2) -> Exit {
	match langs_inner(args, stdout) {
		Ok(()) => Exit::Match,
		Err(e) => {
			let _ = writeln!(stderr, "code-moniker: {e:#}");
			Exit::UsageError
		}
	}
}

fn collect_kinds(
	lang: code_moniker_core::lang::Lang,
) -> Vec<(&'static str, code_moniker_core::core::shape::Shape)> {
	use code_moniker_core::core::shape::Shape;
	language_kinds::known_kinds(std::iter::once(&lang))
		.into_iter()
		.map(|k| (k, Shape::for_kind(k.as_bytes())))
		.collect()
}

fn langs_inner<W: Write>(args: &LangsArgs, stdout: &mut W) -> anyhow::Result<()> {
	use code_moniker_core::lang::Lang;

	match &args.lang {
		None => match args.format {
			LangsFormat::Text => {
				for lang in Lang::ALL {
					writeln!(stdout, "{}", lang.tag())?;
				}
			}
			LangsFormat::Json => {
				let tags: Vec<&str> = Lang::ALL.iter().map(|l| l.tag()).collect();
				serde_json::to_writer_pretty(&mut *stdout, &tags)?;
				stdout.write_all(b"\n")?;
			}
		},
		Some(tag) => {
			let lang = Lang::from_tag(tag).ok_or_else(|| {
				let known: Vec<&str> = Lang::ALL.iter().map(|l| l.tag()).collect();
				anyhow::anyhow!("unknown language `{tag}` (known: {})", known.join(", "))
			})?;
			let kinds = collect_kinds(lang);
			let visibilities = lang.allowed_visibilities();
			match args.format {
				LangsFormat::Text => write_langs_text(stdout, lang.tag(), &kinds, visibilities)?,
				LangsFormat::Json => write_langs_json(stdout, lang.tag(), &kinds, visibilities)?,
			}
		}
	}
	Ok(())
}

fn write_langs_text<W: Write>(
	w: &mut W,
	tag: &str,
	kinds: &[(&'static str, code_moniker_core::core::shape::Shape)],
	visibilities: &[&'static str],
) -> std::io::Result<()> {
	use code_moniker_core::core::shape::Shape;
	writeln!(w, "lang: {tag}")?;
	writeln!(w, "kinds:")?;
	let width = Shape::ALL
		.iter()
		.map(|s| s.as_str().len() + 1)
		.max()
		.unwrap_or(0);
	for shape in Shape::ALL {
		let names: Vec<&str> = kinds
			.iter()
			.filter(|(_, s)| s == shape)
			.map(|(n, _)| *n)
			.collect();
		if names.is_empty() {
			continue;
		}
		writeln!(
			w,
			"  {:<width$} {}",
			format!("{}:", shape.as_str()),
			names.join(", "),
			width = width
		)?;
	}
	if visibilities.is_empty() {
		writeln!(w, "visibilities: (none — ignored by this language)")?;
	} else {
		writeln!(w, "visibilities: {}", visibilities.join(", "))?;
	}
	Ok(())
}

fn write_langs_json<W: Write>(
	w: &mut W,
	tag: &str,
	kinds: &[(&'static str, code_moniker_core::core::shape::Shape)],
	visibilities: &[&'static str],
) -> anyhow::Result<()> {
	#[derive(serde::Serialize)]
	struct KindEntry<'a> {
		name: &'a str,
		shape: &'a str,
	}
	#[derive(serde::Serialize)]
	struct Out<'a> {
		lang: &'a str,
		kinds: Vec<KindEntry<'a>>,
		visibilities: &'a [&'static str],
	}
	let out = Out {
		lang: tag,
		kinds: kinds
			.iter()
			.map(|(n, s)| KindEntry {
				name: n,
				shape: s.as_str(),
			})
			.collect(),
		visibilities,
	};
	serde_json::to_writer_pretty(&mut *w, &out)?;
	w.write_all(b"\n")?;
	Ok(())
}
