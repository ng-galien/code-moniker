mod context;
mod server;
mod tools;

pub(crate) use context::{DaemonRuntime, McpContext};
pub(crate) use server::router;

#[cfg(test)]
mod tests;
