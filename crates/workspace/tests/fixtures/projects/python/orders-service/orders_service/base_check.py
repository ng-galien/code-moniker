import unittest


class BaseCheck(unittest.TestCase):
    def check_label(self) -> str:
        return "base"
