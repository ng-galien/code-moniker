from analytics_service.models import Customer, RiskScore
from analytics_service.policies import RiskPolicy


class AnalyticsService:
    def __init__(self, policy: RiskPolicy) -> None:
        self._policy = policy

    def score(self, customer: Customer, features: dict[str, int]) -> RiskScore:
        return self._policy.evaluate(customer, features)


def build_default_service() -> AnalyticsService:
    return AnalyticsService(RiskPolicy())
