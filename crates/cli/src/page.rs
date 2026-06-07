use crate::args::ExtractArgs;
use code_moniker_core::core::moniker::{Moniker, MonikerBuilder};
use code_moniker_core::core::uri::{UriConfig, from_uri, to_uri};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PageSpec {
	after: Option<Moniker>,
	limit: Option<usize>,
}

impl PageSpec {
	pub(crate) fn from_args(args: &ExtractArgs, scheme: &str) -> anyhow::Result<Self> {
		let cfg = UriConfig { scheme };
		let after = args
			.after
			.as_deref()
			.map(|raw| {
				from_uri(raw, &cfg).map_err(|e| anyhow::anyhow!("invalid --after `{raw}`: {e}"))
			})
			.transpose()?;
		Ok(Self {
			after,
			limit: (!args.all).then_some(args.limit),
		})
	}

	pub(crate) fn allows(&self, moniker: &Moniker) -> bool {
		self.after
			.as_ref()
			.is_none_or(|after| moniker.as_encoded() > after.as_encoded())
	}

	pub(crate) fn page_len(&self, total: usize) -> usize {
		self.limit.map_or(total, |limit| limit.min(total))
	}

	pub(crate) fn info(
		&self,
		total: usize,
		emitted: usize,
		last: Option<&Moniker>,
		scheme: &str,
	) -> PageInfo {
		let remaining = total.saturating_sub(emitted);
		let next_cursor = if remaining > 0 {
			last.map(|moniker| to_uri(moniker, &UriConfig { scheme }))
		} else {
			None
		};
		PageInfo {
			total,
			emitted,
			remaining,
			next_cursor,
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PageInfo {
	pub(crate) total: usize,
	pub(crate) emitted: usize,
	pub(crate) remaining: usize,
	pub(crate) next_cursor: Option<String>,
}

impl PageInfo {
	#[cfg(test)]
	pub(crate) fn unbounded(total: usize) -> Self {
		Self {
			total,
			emitted: total,
			remaining: 0,
			next_cursor: None,
		}
	}
}

pub(crate) struct CursorKey<'a> {
	pub(crate) moniker: &'a Moniker,
	pub(crate) rank: u8,
	pub(crate) rel: &'a [u8],
	pub(crate) source: &'a [u8],
	pub(crate) kind: &'a [u8],
	pub(crate) position: Option<(u32, u32)>,
	pub(crate) ordinal: usize,
}

pub(crate) fn cursor_moniker(key: CursorKey<'_>) -> Moniker {
	let (start, end) = key
		.position
		.map(|(start, end)| (fixed_u32(start), fixed_u32(end)))
		.unwrap_or_else(|| ("-".to_string(), "-".to_string()));
	let mut builder = MonikerBuilder::new();
	builder.project(b".");
	builder.segment(b"cursor", b"v1");
	builder.segment(b"moniker", hex(key.moniker.as_encoded()).as_bytes());
	builder.segment(b"rank", key.rank.to_string().as_bytes());
	builder.segment(b"file", hex(key.rel).as_bytes());
	builder.segment(b"source", hex(key.source).as_bytes());
	builder.segment(b"kind", hex(key.kind).as_bytes());
	builder.segment(b"position", format!("{start}:{end}").as_bytes());
	builder.segment(b"ordinal", fixed_usize(key.ordinal).as_bytes());
	builder.build()
}

pub(crate) fn def_cursor_moniker(
	moniker: &Moniker,
	rel: &[u8],
	kind: &[u8],
	position: Option<(u32, u32)>,
	ordinal: usize,
) -> Moniker {
	cursor_moniker(CursorKey {
		moniker,
		rank: 0,
		rel,
		source: &[],
		kind,
		position,
		ordinal,
	})
}

pub(crate) fn ref_cursor_moniker(
	target: &Moniker,
	rel: &[u8],
	source: &Moniker,
	kind: &[u8],
	position: Option<(u32, u32)>,
	ordinal: usize,
) -> Moniker {
	cursor_moniker(CursorKey {
		moniker: target,
		rank: 1,
		rel,
		source: source.as_encoded(),
		kind,
		position,
		ordinal,
	})
}

fn fixed_u32(n: u32) -> String {
	format!("{n:010}")
}

fn fixed_usize(n: usize) -> String {
	format!("{n:020}")
}

fn hex(bytes: &[u8]) -> String {
	const LUT: &[u8; 16] = b"0123456789abcdef";
	let mut out = String::with_capacity(bytes.len() * 2);
	for b in bytes {
		out.push(LUT[(b >> 4) as usize] as char);
		out.push(LUT[(b & 0x0f) as usize] as char);
	}
	out
}
