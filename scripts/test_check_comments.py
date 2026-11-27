#!/usr/bin/env python3
import importlib.util
import unittest
from pathlib import Path

spec = importlib.util.spec_from_file_location("check_comments", Path(__file__).with_name("check-comments.py"))
check = importlib.util.module_from_spec(spec)
spec.loader.exec_module(check)


class Counting(unittest.TestCase):
    def test_rust_counts_real_comments_only(self):
        src = 'let a = "http://x"; // one\nlet b = r#"// no"#;\nlet c = \'/\';\n/* two */\nlet d = br"//";\n'
        self.assertEqual([line for line, _ in check.comments("x.rs", src)], [1, 4])

    def test_python_counts_docstrings_and_skips_directives(self):
        src = '#!/usr/bin/env python3\n"""doc"""\nimport x  # noqa: E402\ny = "# not"  # yes\n'
        self.assertEqual(len(check.comments("x.py", src)), 2)
        pin = "      - uses: actions/checkout@" + "a" * 40 + " # v4.3.1\n# real\n"
        self.assertEqual(len(check.comments("ci.yml", pin)), 1)


if __name__ == "__main__":
    unittest.main()
