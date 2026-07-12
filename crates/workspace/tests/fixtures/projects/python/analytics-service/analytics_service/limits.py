from .constants import DEFAULT_LIMIT


def effective_limit(requested: int) -> int:
    print(requested)
    if requested <= 0:
        return DEFAULT_LIMIT
    return min(requested, DEFAULT_LIMIT)
