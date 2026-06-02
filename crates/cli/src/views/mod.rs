mod config;
mod model;
mod render;
mod resolve;

#[cfg(feature = "tui")]
pub(crate) use config::load as load_views;
#[cfg(feature = "tui")]
pub(crate) use model::{BoundarySpec, GotchaSpec};
#[cfg(feature = "mcp")]
pub(crate) use model::{MonikerDisplay, RenderOptions};
#[cfg(feature = "mcp")]
pub(crate) use render::{is_views_uri, render_lmnav};
