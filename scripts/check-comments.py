#!/usr/bin/env python3
import argparse
import ast
import io
import json
import re
import subprocess
import sys
import tokenize
from pathlib import Path

DESCRIPTION = (
    "Comment ratchet (docs/adr/0001-no-code-comments.md). Fails when a file has more "
    "comments than scripts/comment-baseline.json allows, or fewer without --update."
)
ROOT = Path(__file__).resolve().parent.parent
BASELINE = ROOT / "scripts/comment-baseline.json"
SUFFIXES = (".rs", ".py", ".sh", ".yml", ".yaml", ".toml")
DIRECTIVE = re.compile(r"#\s*(noqa|type:\s*ignore|shellcheck)\b")
ACTION_PIN = re.compile(r"uses: \S+@[0-9a-f]{40} # v[0-9][\w.-]*\s*$")
RAW_STRING = re.compile(r'(?<![\w])b?r(#*)"')


def rust_comments(src):
    found, i, n = [], 0, len(src)
    while i < n:
        if src.startswith("//", i):
            end = src.find("\n", i)
            end = n if end < 0 else end
            found.append((src.count("\n", 0, i) + 1, src[i:end]))
            i = end
        elif src.startswith("/*", i):
            end, depth, j = n, 1, i + 2
            while j < n and depth:
                if src.startswith("/*", j):
                    depth, j = depth + 1, j + 2
                elif src.startswith("*/", j):
                    depth, j = depth - 1, j + 2
                else:
                    j += 1
            end = j
            found.append((src.count("\n", 0, i) + 1, src[i:end]))
            i = end
        elif raw := RAW_STRING.match(src, i):
            close = '"' + raw.group(1)
            i = src.index(close, raw.end()) + len(close)
        elif src[i] == '"':
            j = i + 1
            while src[j] != '"':
                j += 2 if src[j] == "\\" else 1
            i = j + 1
        elif src[i] == "'" and i + 2 < n and (src[i + 1] == "\\" or src[i + 2] == "'"):
            i = src.index("'", i + 3 if src[i + 1] == "\\" else i + 2) + 1
        else:
            i += 1
    return found


def python_comments(src):
    found = []
    for tok in tokenize.generate_tokens(io.StringIO(src).readline):
        if tok.type == tokenize.COMMENT:
            if tok.start[0] == 1 and tok.string.startswith("#!") or DIRECTIVE.match(tok.string):
                continue
            found.append((tok.start[0], tok.string))
    for node in ast.walk(ast.parse(src)):
        if isinstance(node, (ast.Module, ast.ClassDef, ast.FunctionDef, ast.AsyncFunctionDef)):
            if ast.get_docstring(node) is not None:
                found.append((node.body[0].lineno, "docstring"))
    return found


def hash_comments(src):
    found = []
    for number, line in enumerate(src.splitlines(), 1):
        stripped = line.lstrip()
        if number == 1 and stripped.startswith("#!") or DIRECTIVE.match(stripped) or ACTION_PIN.search(line):
            continue
        if stripped.startswith("#") or re.search(r"\s#\s", line) and not re.search(r"""['"].*\s#\s.*['"]""", line):
            found.append((number, stripped))
    return found


def comments(path, src):
    if path.endswith(".rs"):
        return rust_comments(src)
    if path.endswith(".py"):
        return python_comments(src)
    return hash_comments(src)


def scan(root):
    listed = subprocess.run(["git", "ls-files", "-z"], cwd=root, check=True, capture_output=True).stdout
    counts, where = {}, {}
    for path in sorted(p for p in listed.decode().split("\0") if p.endswith(SUFFIXES)):
        file = root / path
        if file.is_symlink() or not file.is_file():
            continue
        found = comments(path, file.read_text())
        if found:
            counts[path], where[path] = len(found), found
    return counts, where


def main():
    parser = argparse.ArgumentParser(description=DESCRIPTION)
    parser.add_argument("--update", action="store_true", help="lower the baseline to current counts; never raises it")
    parser.add_argument("--list", action="store_true", help="print every counted comment")
    args = parser.parse_args()
    counts, where = scan(ROOT)
    baseline = json.loads(BASELINE.read_text()) if BASELINE.exists() else {}
    if args.list:
        for path, found in where.items():
            for line, text in found:
                print(f"{path}:{line}: {text}")
    seeding = args.update and not BASELINE.exists()
    grown = {} if seeding else {p: (baseline.get(p, 0), c) for p, c in counts.items() if c > baseline.get(p, 0)}
    if grown:
        for path, (allowed, actual) in grown.items():
            print(f"{path}: {actual} comments, baseline allows {allowed}. Move the reason into an ADR or the code.", file=sys.stderr)
        return 1
    shrunk = {p: c for p, c in baseline.items() if counts.get(p, 0) < c}
    if shrunk and not args.update:
        for path, allowed in shrunk.items():
            print(f"{path}: {counts.get(path, 0)} comments, baseline still allows {allowed}. Run: python3 scripts/check-comments.py --update", file=sys.stderr)
        return 1
    if args.update:
        BASELINE.write_text(json.dumps(dict(sorted(counts.items())), indent=2) + "\n")
    print(f"comment ratchet: {sum(counts.values())} comments in {len(counts)} files")
    return 0


if __name__ == "__main__":
    sys.exit(main())
