use std::collections::VecDeque;

// cm: def JobStatus
pub enum JobStatus {
	Queued,
	Running,
	Done,
}

// cm: def Job
pub struct Job {
	pub id: String,
	pub status: JobStatus,
}

// cm: def JobStore
pub trait JobStore {
	// cm: def JobStore.reserve
	fn reserve(&mut self) -> Option<Job>;
	fn complete(&mut self, id: &str);
}

// cm: def MemoryStore
pub struct MemoryStore {
	queue: VecDeque<Job>,
	done: Vec<String>,
}

impl MemoryStore {
	// cm: def MemoryStore.new
	pub fn new(queue: VecDeque<Job>) -> Self {
		Self {
			queue,
			done: Vec::new(),
		}
	}
}

impl JobStore for MemoryStore {
	fn reserve(&mut self) -> Option<Job> {
		self.queue.pop_front()
	}

	fn complete(&mut self, id: &str) {
		self.done.push(id.to_string());
	}
}

// cm: def run_once
pub async fn run_once<S: JobStore>(store: &mut S) -> bool {
	// cm: ref run_once.calls.reserve
	let Some(job) = store.reserve() else {
		return false;
	};
	store.complete(&job.id);
	true
}
