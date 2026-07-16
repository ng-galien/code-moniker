import unittest

from orders_service.base_check import BaseCheck
from orders_service.repository import OrderRepository


class RepositoryChecks(unittest.TestCase):
    def test_open_session(self) -> None:
        repo = OrderRepository("dsn")
        self.assertEqual(repo.open_session(), "session:dsn")


class LayeredChecks(BaseCheck):
    def test_layered_label(self) -> None:
        self.assertIn("base", self.check_label())
