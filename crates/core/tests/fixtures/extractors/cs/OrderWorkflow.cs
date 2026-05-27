namespace App.Order;

using System;
using System.Collections.Generic;
using System.Linq;
using System.Threading;
using System.Threading.Tasks;

// cm: def OrderStatus
public enum OrderStatus
{
	Draft,
	Paid,
	Cancelled
}

// cm: def Order
public record Order(string Id, OrderStatus Status, DateTimeOffset CreatedAt);

// cm: def IOrderRepository
public interface IOrderRepository
{
	Task<IReadOnlyList<Order>> FindExpiredAsync(DateTimeOffset now, CancellationToken token);
	Task CancelAsync(string id, CancellationToken token);
}

// cm: def OrderWorkflow
public sealed class OrderWorkflow
{
	private readonly IOrderRepository _repository;
	private readonly TimeProvider _timeProvider;

	public OrderWorkflow(IOrderRepository repository, TimeProvider timeProvider)
	{
		_repository = repository;
		_timeProvider = timeProvider;
	}

	// cm: def OrderWorkflow.CancelExpiredAsync
	public async Task<int> CancelExpiredAsync(CancellationToken token)
	{
		// cm: ref OrderWorkflow.CancelExpiredAsync.calls.FindExpiredAsync
		var expired = await _repository.FindExpiredAsync(_timeProvider.GetUtcNow(), token);
		foreach (var order in expired.Where(o => o.Status == OrderStatus.Draft))
		{
			await _repository.CancelAsync(order.Id, token);
		}
		return expired.Count;
	}
}
