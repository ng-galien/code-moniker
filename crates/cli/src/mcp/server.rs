use std::io::Write;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde_json::{Value, json};

use crate::session::SessionOptions;

use super::context::McpContext;
use super::{dispatch, transport};

pub(crate) struct McpServer {
	addr: SocketAddr,
	running: Arc<AtomicBool>,
	thread: Option<JoinHandle<()>>,
}

impl McpServer {
	pub(crate) fn endpoint(&self) -> String {
		format!("http://{}/mcp", self.addr)
	}
}

impl Drop for McpServer {
	fn drop(&mut self) {
		self.running.store(false, Ordering::Release);
		let _ = TcpStream::connect(self.addr);
		if let Some(thread) = self.thread.take() {
			let _ = thread.join();
		}
	}
}

pub(crate) fn start(opts: SessionOptions, scheme: String, port: u16) -> anyhow::Result<McpServer> {
	let listener = TcpListener::bind(("127.0.0.1", port))?;
	listener.set_nonblocking(true)?;
	let addr = listener.local_addr()?;
	let running = Arc::new(AtomicBool::new(true));
	let thread_running = Arc::clone(&running);
	let context = McpContext::new(opts, scheme);
	let thread = thread::spawn(move || serve(listener, thread_running, context));
	Ok(McpServer {
		addr,
		running,
		thread: Some(thread),
	})
}

fn serve(listener: TcpListener, running: Arc<AtomicBool>, context: McpContext) {
	while running.load(Ordering::Acquire) {
		match listener.accept() {
			Ok((stream, _)) => handle_stream(stream, &context),
			Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
				thread::sleep(Duration::from_millis(25));
			}
			Err(_) => break,
		}
	}
}

fn handle_stream(mut stream: TcpStream, context: &McpContext) {
	let response = transport::read_http_request(&mut stream)
		.and_then(|request| handle_http_request(request, context))
		.unwrap_or_else(transport::error_response);
	let _ = stream.write_all(response.as_bytes());
	let _ = stream.flush();
}

fn handle_http_request(
	request: transport::HttpRequest,
	context: &McpContext,
) -> anyhow::Result<String> {
	if request.method == "OPTIONS" {
		return Ok(transport::http_response(204, ""));
	}
	if request.method != "POST" || request.path != transport::MCP_PATH {
		return Ok(transport::http_json(
			404,
			json!({"error": "not_found", "path": request.path}),
		));
	}
	let request: Value = serde_json::from_slice(&request.body)?;
	let response = dispatch::handle_json_rpc(&request, context);
	Ok(transport::http_json(200, response))
}

#[cfg(test)]
pub(super) fn server_addr(server: &McpServer) -> SocketAddr {
	server.addr
}
