/*
 * User account service.
 *
 * The repository is injected so the service can be exercised against an
 * in-memory implementation in tests and against a JPA-backed one in prod.
 */
package app.user;

import java.util.List;
import java.util.Optional;
import java.util.concurrent.ConcurrentHashMap;
import java.util.stream.Collectors;
import java.util.stream.Stream;

/**
 * Business logic + reference repository implementation for user accounts.
 *
 * Keeping the impl as an inner class avoids polluting the package with a
 * second top-level type for a piece used only in tests.
 */
public class UserService {

	/** Immutable user record. Tags are defensively copied on construction. */
	public record User(String id, String email, String name, List<String> tags) {}

	/** Surfaced by the controller as 404. */
	public static class UserNotFoundException extends RuntimeException {
		public UserNotFoundException(String id) {
			super("user " + id + " not found");
		}
	}

	/** Surfaced by the controller as 409 with the offending field. */
	public static class ConflictException extends RuntimeException {
		private final String field;
		private final String value;

		public ConflictException(String field, String value) {
			super("conflict on " + field + "=" + value);
			this.field = field;
			this.value = value;
		}

		public String field() {
			return field;
		}

		public String value() {
			return value;
		}
	}

	/**
	 * Storage seam. Implementations must be safe for concurrent use because
	 * the HTTP runtime serves requests from a thread pool.
	 */
	public interface UserRepository {
		Optional<User> findById(String id);
		Optional<User> findByEmail(String email);
		User insert(User user);
		Stream<User> scan();
	}

	/**
	 * Reference impl backed by ConcurrentHashMap. Suitable for tests and
	 * dev — production binds the JPA-backed implementation instead.
	 */
	public static class InMemoryRepository implements UserRepository {
		private final ConcurrentHashMap<String, User> byId = new ConcurrentHashMap<>();

		@Override
		public Optional<User> findById(String id) {
			return Optional.ofNullable(byId.get(id));
		}

		@Override
		public Optional<User> findByEmail(String email) {
			// Linear scan — fine for tests; the JPA impl uses an index.
			return byId.values().stream()
				.filter(u -> u.email().equals(email))
				.findFirst();
		}

		@Override
		public User insert(User user) {
			// FIXME: race between findByEmail and put. Acceptable for the
			// in-memory impl used in tests; revisit if it becomes prod.
			if (findByEmail(user.email()).isPresent()) {
				throw new ConflictException("email", user.email());
			}
			byId.put(user.id(), user);
			return user;
		}

		@Override
		public Stream<User> scan() {
			return byId.values().stream();
		}
	}

	private final UserRepository repo;

	public UserService(UserRepository repo) {
		this.repo = repo;
	}

	/** @throws UserNotFoundException when no user matches {@code id}. */
	public User get(String id) {
		return repo.findById(id).orElseThrow(() -> new UserNotFoundException(id));
	}

	/**
	 * Create a new user. Email must be unique.
	 *
	 * @throws ConflictException when the email is already taken.
	 */
	public User create(String email, String name, List<String> tags) {
		if (repo.findByEmail(email).isPresent()) {
			throw new ConflictException("email", email);
		}
		User user = new User(makeId(email), email, name, List.copyOf(tags));
		return repo.insert(user);
	}

	public List<User> withTag(String tag) {
		return repo.scan()
			.filter(u -> u.tags().contains(tag))
			.collect(Collectors.toList());
	}

	// Local part of the email, lowercased. Total — caller cannot pass null.
	private static String makeId(String email) {
		int at = email.indexOf('@');
		return at > 0 ? email.substring(0, at).toLowerCase() : email.toLowerCase();
	}
}
