use code_moniker_core::core::code_graph::{DefRecord, RefRecord};
use code_moniker_core::core::moniker::Moniker;
use code_moniker_core::lang::Lang;

pub(crate) fn is_navigable_def(lang: Lang, def: &DefRecord) -> bool {
	lang.kind_spec(&def_kind(def)).is_some()
}

pub(crate) fn def_kind(def: &DefRecord) -> String {
	std::str::from_utf8(&def.kind).unwrap_or("?").to_string()
}

pub(crate) fn ref_kind(reference: &RefRecord) -> String {
	std::str::from_utf8(&reference.kind)
		.unwrap_or("?")
		.to_string()
}

pub(crate) fn last_name(moniker: &Moniker) -> String {
	moniker
		.as_view()
		.segments()
		.last()
		.and_then(|s| std::str::from_utf8(s.name).ok())
		.unwrap_or(".")
		.to_string()
}

pub(crate) fn compact_moniker(moniker: &Moniker) -> String {
	crate::format::render_compact_moniker(moniker, false).unwrap_or_else(|| {
		let cfg = code_moniker_core::core::uri::UriConfig {
			scheme: crate::DEFAULT_SCHEME,
		};
		crate::render_uri(moniker, &cfg)
	})
}
