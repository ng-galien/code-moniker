//! In-memory user repository.
//!
//! The trait `UserRepository` is the seam every consumer codes against.
//! `InMemoryRepo` is the test/dev implementation; production binds the
//! same trait to a Postgres-backed impl in `crate::store::pg`.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::RwLock;

/// Snapshot of a user as the repository sees it. Cloned freely on read —
/// the in-memory map owns the canonical copy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct User {
	pub id: String,
	pub email: String,
	pub name: String,
	pub tags: Vec<String>,
}

/// Errors a `UserRepository` can produce.
///
/// `Internal` wraps lock-poisoning failures because callers typically can't
/// recover from those — they bubble up as 500s.
#[derive(Debug)]
pub enum RepoError {
	/// No user matched the lookup key.
	NotFound,
	/// Uniqueness violation; the inner string names the conflicting value.
	Conflict(String),
	/// Lock poisoning, allocation failures, etc.
	Internal(String),
}

impl std::fmt::Display for RepoError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			RepoError::NotFound => write!(f, "not found"),
			RepoError::Conflict(s) => write!(f, "conflict: {s}"),
			RepoError::Internal(s) => write!(f, "internal: {s}"),
		}
	}
}

impl std::error::Error for RepoError {}

/// Storage interface. Implementors must be `Send + Sync` because the HTTP
/// runtime calls into them from multiple threads.
pub trait UserRepository: Send + Sync {
	fn find_by_id(&self, id: &str) -> Result<Option<User>, RepoError>;
	fn find_by_email(&self, email: &str) -> Result<Option<User>, RepoError>;
	fn insert(&self, user: User) -> Result<User, RepoError>;
	/// Iterate over every user. Order is unspecified.
	fn scan<'a>(&'a self) -> Box<dyn Iterator<Item = User> + 'a>;
}

/// Reference implementation backed by a `RwLock<HashMap>`.
///
/// Suitable for tests and dev — production should use the Postgres impl.
pub struct InMemoryRepo {
	by_id: Arc<RwLock<HashMap<String, User>>>,
}

impl InMemoryRepo {
	pub fn new() -> Self {
		Self {
			by_id: Arc::new(RwLock::new(HashMap::new())),
		}
	}

	// Centralised lock acquisition: every read site goes through here so
	// poisoning surfaces as `RepoError::Internal` once, not at every callsite.
	fn read_guard(
		&self,
	) -> Result<std::sync::RwLockReadGuard<'_, HashMap<String, User>>, RepoError> {
		self.by_id
			.read()
			.map_err(|e| RepoError::Internal(e.to_string()))
	}
}

impl Default for InMemoryRepo {
	fn default() -> Self {
		Self::new()
	}
}

impl UserRepository for InMemoryRepo {
	fn find_by_id(&self, id: &str) -> Result<Option<User>, RepoError> {
		Ok(self.read_guard()?.get(id).cloned())
	}

	fn find_by_email(&self, email: &str) -> Result<Option<User>, RepoError> {
		// Linear scan — fine for the in-memory impl, the Postgres impl
		// uses an index on `email`.
		let by_id = self.read_guard()?;
		Ok(by_id.values().find(|u| u.email == email).cloned())
	}

	fn insert(&self, user: User) -> Result<User, RepoError> {
		let mut by_id = self
			.by_id
			.write()
			.map_err(|e| RepoError::Internal(e.to_string()))?;
		// Email uniqueness is enforced here, not at the type level, because
		// the schema migration that added the unique index landed in v3.2.
		if by_id.values().any(|u| u.email == user.email) {
			return Err(RepoError::Conflict(format!("email {} taken", user.email)));
		}
		by_id.insert(user.id.clone(), user.clone());
		Ok(user)
	}

	fn scan<'a>(&'a self) -> Box<dyn Iterator<Item = User> + 'a> {
		// Snapshot up front so we don't hold the read guard across the
		// returned iterator — that would deadlock writers indefinitely.
		let snapshot: Vec<User> = match self.read_guard() {
			Ok(g) => g.values().cloned().collect(),
			Err(_) => Vec::new(),
		};
		Box::new(snapshot.into_iter())
	}
}

/// Convenience — returns every user wearing `tag`. Allocates; for hot
/// paths use `repo.scan()` directly.
pub fn users_with_tag<R: UserRepository + ?Sized>(repo: &R, tag: &str) -> Vec<User> {
	repo.scan()
		.filter(|u| u.tags.iter().any(|t| t == tag))
		.collect()
}

/// Stable id derived from the email's local part.
///
/// FIXME: collides if two users share the local part on different domains.
/// We accepted that for now because tenants are siloed by domain at the
/// HTTP layer; revisit when multi-domain tenants land.
pub fn make_id(email: &str) -> String {
	match email.find('@') {
		Some(at) if at > 0 => email[..at].to_lowercase(),
		_ => email.to_lowercase(),
	}
}
