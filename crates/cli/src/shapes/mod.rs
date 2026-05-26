use std::io::Write;

use crate::Exit;
use crate::args::{LangsFormat, ShapesArgs};

pub fn run<W1: Write, W2: Write>(args: &ShapesArgs, stdout: &mut W1, stderr: &mut W2) -> Exit {
	match shapes_inner(args, stdout) {
		Ok(()) => Exit::Match,
		Err(e) => {
			let _ = writeln!(stderr, "code-moniker: {e:#}");
			Exit::UsageError
		}
	}
}

fn shape_description(shape: code_moniker_core::core::shape::Shape) -> &'static str {
	use code_moniker_core::core::shape::Shape;
	match shape {
		Shape::Namespace => "container scopes (module, namespace, schema, impl)",
		Shape::Type => {
			"type-like declarations (class, struct, enum, interface, trait, table, view, ...)"
		}
		Shape::Callable => {
			"executable code (function, method, constructor, procedure, async_function)"
		}
		Shape::Value => "named bindings (field, const, static, enum_constant, param, local, ...)",
		Shape::Annotation => "attached metadata (comment) - not a structural scope",
		Shape::Ref => {
			"cross-record references (calls, imports_*, extends, uses_type, ...) - marker shape for ref records"
		}
	}
}

fn shapes_inner<W: Write>(args: &ShapesArgs, stdout: &mut W) -> anyhow::Result<()> {
	use code_moniker_core::core::shape::Shape;
	match args.format {
		LangsFormat::Text => {
			writeln!(
				stdout,
				"Each def's `kind` maps to exactly one shape; refs share `ref` as marker."
			)?;
			writeln!(
				stdout,
				"Filter with `--shape <NAME>`; `code-moniker langs <TAG>` shows the kind<->shape map per language."
			)?;
			writeln!(stdout)?;
			let width = Shape::ALL
				.iter()
				.map(|s| s.as_str().len())
				.max()
				.unwrap_or(0);
			for shape in Shape::ALL {
				writeln!(
					stdout,
					"  {:<width$}  {}",
					shape.as_str(),
					shape_description(*shape),
					width = width
				)?;
			}
		}
		LangsFormat::Json => {
			#[derive(serde::Serialize)]
			struct Entry<'a> {
				name: &'a str,
				description: &'a str,
			}
			let entries: Vec<Entry> = Shape::ALL
				.iter()
				.map(|s| Entry {
					name: s.as_str(),
					description: shape_description(*s),
				})
				.collect();
			serde_json::to_writer_pretty(&mut *stdout, &entries)?;
			stdout.write_all(b"\n")?;
		}
	}
	Ok(())
}
