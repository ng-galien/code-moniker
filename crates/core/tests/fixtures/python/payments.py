"""Payment orchestration for checkout flows."""

from __future__ import annotations

from collections.abc import Awaitable, Callable, Mapping, Sequence
from dataclasses import dataclass, field
from decimal import Decimal
from enum import StrEnum
from typing import Protocol, TypeAlias, TypedDict


# cm: def Metadata
type Metadata = Mapping[str, object]

# cm: def WebhookPayload
WebhookPayload: TypeAlias = dict[str, object]


# cm: def PaymentStatus
class PaymentStatus(StrEnum):
    PENDING = "pending"
    CAPTURED = "captured"


# cm: def PaymentEvent
class PaymentEvent(TypedDict, total=False):
    id: str
    amount: str
    metadata: Metadata


@dataclass(slots=True)
# cm: def Payment
class Payment:
    id: str
    amount: Decimal
    status: PaymentStatus
    metadata: Metadata = field(default_factory=dict)


# cm: def PaymentGateway
class PaymentGateway(Protocol):
    async def authorize(self, payment: Payment) -> PaymentEvent: ...
    async def capture(self, payment_id: str) -> PaymentEvent: ...


# cm: def AuditSink
AuditSink: TypeAlias = Callable[[PaymentEvent], Awaitable[None]]


class PaymentService:
    def __init__(self, gateway: PaymentGateway, audit: AuditSink) -> None:
        self._gateway = gateway
        self._audit = audit

    # cm: def PaymentService.authorize_order
    async def authorize_order(
        self,
        order_id: str,
        amount: Decimal,
        tags: Sequence[str],
    ) -> Payment:
        # cm: ref PaymentService.authorize_order.instantiates.Payment
        payment = Payment(
            id=order_id,
            amount=amount,
            status=PaymentStatus.PENDING,
            metadata={"tags": list(tags)},
        )
        # cm: ref PaymentService.authorize_order.calls.PaymentGateway.authorize
        event = await self._gateway.authorize(payment)
        # cm: ref PaymentService.authorize_order.calls.AuditSink
        await self._audit(event)
        return payment

    # cm: def PaymentService.capture
    async def capture(self, payment: Payment) -> PaymentEvent:
        if payment.status is PaymentStatus.CAPTURED:
            return {"id": payment.id}
        # cm: ref PaymentService.capture.calls.PaymentGateway.capture
        return await self._gateway.capture(payment.id)
