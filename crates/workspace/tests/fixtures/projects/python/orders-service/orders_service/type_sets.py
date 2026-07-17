class AlphaRenderer:
	def render(self):
		return "alpha"


class BetaRenderer:
	def render(self):
		return "beta"


def render_union(value: AlphaRenderer | BetaRenderer):
	return value.render()


def render_optional(value: AlphaRenderer | None):
	return value.render()


def render_reassigned(enabled):
	value = AlphaRenderer()
	if enabled:
		value = BetaRenderer()
	return value.render()


def make_renderer(enabled) -> AlphaRenderer | BetaRenderer:
	if enabled:
		return AlphaRenderer()
	return BetaRenderer()


def render_chained(enabled):
	return make_renderer(enabled).render()


def render_constructed():
	return AlphaRenderer().render()


def render_loop():
	for value in [AlphaRenderer(), BetaRenderer()]:
		value.render()


def render_heterogeneous_tuple(values: tuple[AlphaRenderer, BetaRenderer]):
	for value in values:
		value.render()


class AlphaError(Exception):
	def render(self):
		return "alpha error"


class BetaError(Exception):
	def render(self):
		return "beta error"


def render_exception(error_kind):
	try:
		raise error_kind()
	except (AlphaError, BetaError) as error:
		return error.render()


class FullProtocol:
	def begins(self):
		return True

	def finishes(self):
		return True


class PrefixOnly:
	def begins(self):
		return True


def render_protocol(value):
	value.begins()
	value.finishes()


def render_open(value):
	value.runtime_only()


def read_open():
	return runtime_value
