import unittest

import importlib.util
from pathlib import Path


MODULE_PATH = Path(__file__).with_name("parse_comment_command.py")
SPEC = importlib.util.spec_from_file_location("parse_comment_command", MODULE_PATH)
MODULE = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(MODULE)


class ParseCommentCommandTests(unittest.TestCase):
    def test_parses_agent_from_first_line(self):
        self.assertEqual(MODULE.parse_agent("/engineer fix it\nextra"), "engineer")

    def test_ignores_non_command_comment(self):
        self.assertEqual(MODULE.parse_agent("please help"), "")

    def test_ignores_invalid_agent_name(self):
        self.assertEqual(MODULE.parse_agent("/Engineer uppercase"), "")


if __name__ == "__main__":
    unittest.main()