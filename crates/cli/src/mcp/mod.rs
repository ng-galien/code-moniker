mod context;
mod server;
mod supervisor;
mod tools;

pub(crate) use context::{DaemonRuntime, McpContext};
pub(crate) use server::{router, serve_stdio};
pub(crate) use supervisor::supervise_stdio;

#[cfg(test)]
mod tests;
