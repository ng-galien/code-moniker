use std::ffi::CString;

use pgrx::guc::{GucContext, GucFlags, GucRegistry, GucSetting};

use crate::core::uri::UriConfig;

const FALLBACK_SCHEME: &str = "pcm+moniker://";

static SCHEME: GucSetting<Option<CString>> =
	GucSetting::<Option<CString>>::new(Some(c"pcm+moniker://"));

pub(crate) fn init_gucs() {
	GucRegistry::define_string_guc(
		c"pg_code_moniker.scheme",
		c"Base scheme used by moniker text I/O and by the URI parser.",
		c"Caller-supplied scheme for the moniker SQL type. The +moniker suffix is part of the scheme: \
		  for example pcm+moniker://, esac+moniker://. Setting takes effect on the next moniker_in / \
		  moniker_out call; existing moniker values are byte-for-byte unchanged.",
		&SCHEME,
		GucContext::Userset,
		GucFlags::default(),
	);
}

pub(crate) fn with_current_config<R>(f: impl FnOnce(&UriConfig<'_>) -> R) -> R {
	let raw = SCHEME.get();
	let scheme = raw
		.as_ref()
		.and_then(|c| c.to_str().ok())
		.unwrap_or(FALLBACK_SCHEME);
	f(&UriConfig { scheme })
}

pub(crate) fn current_scheme_owned() -> String {
	let raw = SCHEME.get();
	raw.as_ref()
		.and_then(|c| c.to_str().ok())
		.unwrap_or(FALLBACK_SCHEME)
		.to_string()
}
