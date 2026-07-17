enabled = True


class ConditionalExport:
	pass


class FallbackExport:
	pass


if enabled:
	__all__ = ["ConditionalExport"]
else:
	__all__ = ["FallbackExport"]
