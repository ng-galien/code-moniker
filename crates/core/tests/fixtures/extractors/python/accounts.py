"""User accounts service.

This module owns the business logic for the user resource. Persistence is
abstracted behind the :class:`UserRepository` Protocol so the service can
be exercised against the in-memory implementation in tests and against a
real database in production.

The service does NOT validate input formats — that's the controller's
job, run via Pydantic before we even get here.
"""

from __future__ import annotations

import logging
from dataclasses import dataclass, field
from typing import Iterable, Iterator, Optional, Protocol

# Module-level logger. Configured by the application; tests capture via caplog.
log = logging.getLogger(__name__)


@dataclass(frozen=True)
class User:
    """Immutable user record.

    The `tags` list is stored as an ordinary list (not a tuple) so callers
    that build users by hand don't have to convert; the `frozen=True` only
    prevents reassignment of the fields, not mutation of `tags`. Treat
    `tags` as conceptually frozen anyway.
    """

    id: str
    email: str
    name: str
    tags: list[str] = field(default_factory=list)


class UserNotFoundError(Exception):
    """Raised when a lookup by id returns nothing.

    Surfaced by the HTTP layer as 404. The id is exposed on the exception
    so handlers can include it in the response body.
    """

    def __init__(self, user_id: str) -> None:
        super().__init__(f"user {user_id} not found")
        self.user_id = user_id


class ConflictError(Exception):
    """Raised when a unique constraint would be violated."""

    def __init__(self, field_name: str, value: str) -> None:
        super().__init__(f"conflict on {field_name}={value!r}")
        self.field_name = field_name
        self.value = value


class UserRepository(Protocol):
    """Storage interface — see ``InMemoryRepository`` for the canonical impl."""

    def find_by_id(self, user_id: str) -> Optional[User]: ...
    def find_by_email(self, email: str) -> Optional[User]: ...
    def insert(self, user: User) -> User: ...
    def scan(self) -> Iterator[User]: ...


class UserService:
    """Business logic, injected with a repository at construction time."""

    def __init__(self, repo: UserRepository) -> None:
        # Underscore prefix marks this as internal; the rest of the codebase
        # accesses the repo only via service methods.
        self._repo = repo

    def get(self, user_id: str) -> User:
        user = self._repo.find_by_id(user_id)
        if user is None:
            raise UserNotFoundError(user_id)
        return user

    def create(self, email: str, name: str, tags: Iterable[str] = ()) -> User:
        # Uniqueness check before insert. The DB also has a unique index but
        # we want the structured ConflictError, not whatever the driver raises.
        if self._repo.find_by_email(email) is not None:
            raise ConflictError("email", email)
        new_user = User(id=make_id(email), email=email, name=name, tags=list(tags))
        log.info("creating user", extra={"email": email})
        return self._repo.insert(new_user)

    def with_tag(self, tag: str) -> list[User]:
        # Materialised list is fine — the tag-search endpoint paginates upstream.
        return [u for u in self._repo.scan() if tag in u.tags]


class InMemoryRepository:
    """Reference implementation. Thread-unsafe — wrap in a lock for prod use."""

    def __init__(self) -> None:
        self._by_id: dict[str, User] = {}

    def find_by_id(self, user_id: str) -> Optional[User]:
        return self._by_id.get(user_id)

    def find_by_email(self, email: str) -> Optional[User]:
        for user in self._by_id.values():
            if user.email == email:
                return user
        return None

    def insert(self, user: User) -> User:
        # TODO: race condition between this check and the assignment below;
        # acceptable for the in-memory test impl but real impls must use
        # `INSERT ... ON CONFLICT` or a transaction.
        if self.find_by_email(user.email) is not None:
            raise ConflictError("email", user.email)
        self._by_id[user.id] = user
        return user

    def scan(self) -> Iterator[User]:
        # Yielding lazily means callers can short-circuit without copying.
        yield from self._by_id.values()


def make_id(email: str) -> str:
    """Derive a stable id from the local part of the email.

    Falls back to the full address when no ``@`` is present; the schema
    upstream rejects that case but defending here keeps the function total.
    """
    head, _, _ = email.partition("@")
    return head.lower() if head else email.lower()
