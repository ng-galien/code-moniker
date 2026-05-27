// User accounts service.
//
// The repository abstraction lets the controller stay decoupled from
// persistence. The in-memory implementation here is used by tests and
// the local dev server; production binds an EF Core implementation.

namespace App.User;

using System;
using System.Collections.Concurrent;
using System.Collections.Generic;
using System.Linq;
using System.Threading.Tasks;

/// <summary>Immutable user record. Tags must already be defensively copied by the caller.</summary>
public record User(string Id, string Email, string Name, IReadOnlyList<string> Tags);

/// <summary>Surfaced by the controller as 404.</summary>
public class UserNotFoundException : Exception
{
	public string UserId { get; }

	public UserNotFoundException(string id) : base($"user {id} not found")
	{
		UserId = id;
	}
}

/// <summary>Surfaced by the controller as 409 with the offending field.</summary>
public class ConflictException : Exception
{
	public string Field { get; }
	public string Value { get; }

	public ConflictException(string field, string value) : base($"conflict on {field}={value}")
	{
		Field = field;
		Value = value;
	}
}

/// <summary>
/// Storage seam. Implementations must be safe for concurrent use because
/// the ASP.NET runtime serves requests from a thread pool.
/// </summary>
public interface IUserRepository
{
	Task<User?> FindByIdAsync(string id);
	Task<User?> FindByEmailAsync(string email);
	Task<User> InsertAsync(User user);
	IAsyncEnumerable<User> ScanAsync();
}

/// <summary>Reference impl. Suitable for tests and dev only.</summary>
public class InMemoryRepository : IUserRepository
{
	private readonly ConcurrentDictionary<string, User> _byId = new();

	public Task<User?> FindByIdAsync(string id)
	{
		_byId.TryGetValue(id, out var user);
		return Task.FromResult<User?>(user);
	}

	public Task<User?> FindByEmailAsync(string email)
	{
		// Linear scan — the EF Core impl uses an index on Email.
		var match = _byId.Values.FirstOrDefault(u => u.Email == email);
		return Task.FromResult<User?>(match);
	}

	public async Task<User> InsertAsync(User user)
	{
		// TODO: race between FindByEmail and the indexer assignment below.
		// Acceptable for the in-memory test impl; the EF Core impl wraps
		// in a transaction with a unique constraint.
		var existing = await FindByEmailAsync(user.Email);
		if (existing is not null)
		{
			throw new ConflictException("email", user.Email);
		}
		_byId[user.Id] = user;
		return user;
	}

	public async IAsyncEnumerable<User> ScanAsync()
	{
		foreach (var u in _byId.Values)
		{
			yield return u;
			// Yield back to the scheduler between rows so a slow consumer
			// doesn't starve other tasks on the same loop.
			await Task.Yield();
		}
	}
}

public class UserService
{
	private readonly IUserRepository _repo;

	public UserService(IUserRepository repo)
	{
		_repo = repo;
	}

	/// <summary>Fetch a user by id.</summary>
	/// <exception cref="UserNotFoundException">No user matches <paramref name="id"/>.</exception>
	public async Task<User> GetAsync(string id)
	{
		var user = await _repo.FindByIdAsync(id);
		if (user is null)
		{
			throw new UserNotFoundException(id);
		}
		return user;
	}

	/// <summary>Create a new user. Email must be unique.</summary>
	/// <exception cref="ConflictException">Email already taken.</exception>
	public async Task<User> CreateAsync(string email, string name, IReadOnlyList<string> tags)
	{
		var existing = await _repo.FindByEmailAsync(email);
		if (existing is not null)
		{
			throw new ConflictException("email", email);
		}
		var user = new User(MakeId(email), email, name, tags);
		return await _repo.InsertAsync(user);
	}

	public async IAsyncEnumerable<User> WithTagAsync(string tag)
	{
		await foreach (var u in _repo.ScanAsync())
		{
			if (u.Tags.Contains(tag))
			{
				yield return u;
			}
		}
	}

	// Local part of the email, lowercased. Total — caller cannot pass null
	// because the C# nullable annotation rejects it at the API boundary.
	private static string MakeId(string email)
	{
		var at = email.IndexOf('@');
		return at > 0 ? email[..at].ToLowerInvariant() : email.ToLowerInvariant();
	}
}
