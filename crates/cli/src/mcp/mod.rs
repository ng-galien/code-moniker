mod context;
mod server;
mod tools;

pub(crate) use context::{DaemonRuntime, McpContext};
pub(crate) use server::{router, serve_stdio};

#[cfg(test)]
mod tests;
