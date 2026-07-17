from orders_service.wildcard_impl import ExportedClient

__all__ = ["ExportedClient"]


def package_helper(value: str) -> str:
	return value


def _private_helper(value: str) -> str:
	return value
