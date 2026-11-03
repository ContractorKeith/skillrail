"""Keep the Darwin PTY exception narrower than actual terminal restoration."""
import copy
import sys
import termios
import unittest
from unittest.mock import patch

sys.dont_write_bytecode = True
from smoke import terminal_attributes_equal


class TerminalAttributesTest(unittest.TestCase):
    def setUp(self):
        self.before = [0, 0, 0, termios.ICANON | termios.ECHO, 9600, 9600,
                       [b"\x03", b"\x04"]]

    def test_darwin_accepts_only_pending_input_state(self):
        after = copy.deepcopy(self.before)
        after[3] |= termios.PENDIN
        saved = copy.deepcopy(after)
        with patch("smoke.sys.platform", "darwin"):
            self.assertTrue(terminal_attributes_equal(self.before, after))
            self.assertTrue(terminal_attributes_equal(after, self.before))
        self.assertEqual(after, saved)

    def test_linux_keeps_pending_input_strict(self):
        after = copy.deepcopy(self.before)
        after[3] |= termios.PENDIN
        with patch("smoke.sys.platform", "linux"):
            self.assertFalse(terminal_attributes_equal(self.before, after))

    def test_darwin_rejects_other_flags_speeds_and_control_characters(self):
        changes = [(0, 1), (1, 1), (2, 1), (3, termios.ECHO),
                   (4, 19200), (5, 19200), (6, [b"\x02", b"\x04"])]
        for field, value in changes:
            with self.subTest(field=field), patch("smoke.sys.platform", "darwin"):
                after = copy.deepcopy(self.before)
                after[3] |= termios.PENDIN
                after[field] = value
                self.assertFalse(terminal_attributes_equal(self.before, after))


if __name__ == "__main__":
    unittest.main()
