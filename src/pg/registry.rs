//! URI configuration for the pg layer.

use crate::core::uri::UriConfig;

pub(super) const DEFAULT_CONFIG: UriConfig<'static> = UriConfig {
	scheme: "esac+moniker://",
};
