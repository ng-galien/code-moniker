pub(crate) fn render_uri(
	m: &code_moniker_core::core::moniker::Moniker,
	cfg: &code_moniker_core::core::uri::UriConfig<'_>,
) -> String {
	code_moniker_core::core::uri::to_uri(m, cfg)
		.unwrap_or_else(|_| format!("<non-utf8:{}b>", m.as_bytes().len()))
}
