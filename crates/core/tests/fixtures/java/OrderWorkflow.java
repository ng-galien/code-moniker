package app.order;

import java.time.Clock;
import java.time.Instant;
import java.util.List;

// cm: def OrderWorkflow
public final class OrderWorkflow implements Runnable {
	private final OrderRepository repository;
	private final Clock clock;

	public OrderWorkflow(OrderRepository repository, Clock clock) {
		this.repository = repository;
		this.clock = clock;
	}

	@Override
	// cm: def OrderWorkflow.run
	public void run() {
		// cm: ref OrderWorkflow.run.calls.findExpired
		List<Order> expired = repository.findExpired(clock.instant());
		expired.forEach(order -> repository.cancel(order.id()));
	}

	// cm: def OrderWorkflow.Status
	public enum Status {
		DRAFT,
		PAID,
		CANCELLED
	}

	// cm: def OrderWorkflow.Order
	public record Order(String id, Status status, Instant createdAt) {}

	// cm: def OrderWorkflow.OrderRepository
	public interface OrderRepository {
		List<Order> findExpired(Instant now);
		void cancel(String id);
	}
}
