try:
    from orders_service.conditional_a import ConditionalClient
except ImportError:
    from orders_service.conditional_b import ConditionalClient

__all__ = ["ConditionalClient"]


def build_conditional_client():
	return ConditionalClient("module-runtime")
