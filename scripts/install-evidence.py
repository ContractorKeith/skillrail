#!/usr/bin/env python3
"""Optional pinned Codex metadata-function evidence; not a native loading test."""
import hashlib
import os
from pathlib import Path
import subprocess
import tempfile
import urllib.request

REVISION = "6ff1e1ca3b30fc8b914720681fcf5fe62e95f856"
URL = f"https://raw.githubusercontent.com/openai/codex/{REVISION}/codex-rs/skills/src/parser.rs"
SHA256 = "f3532df1cc16f4da423b8e5c813269940c90317bcd211b6659cad449fd877e89"


def main():
    raw = urllib.request.urlopen(URL, timeout=20).read()
    assert hashlib.sha256(raw).hexdigest() == SHA256, "pinned parser source hash changed"
    source = raw.decode()
    assert "const MAX_NAME_LEN: usize = 64;" in source
    sanitize = source.split("fn sanitize_single_line(", 1)[1].split("fn repair_frontmatter_scalar_fields(", 1)[0]
    validate = source.split("fn validate_len(", 1)[1].split("fn extract_frontmatter(", 1)[0]
    functions = "fn sanitize_single_line(" + sanitize + "\nfn validate_len(" + validate
    # Collaborator has the native error variants/fields consumed by the extracted function.
    program = r'''
#[allow(dead_code)]
#[derive(Debug)]
enum SkillParseError { MissingField(&'static str), InvalidField { field: &'static str, reason: String } }
''' + functions + r'''
fn main() {
    assert!(validate_len(&"n".repeat(64),64,"name").is_ok());
    assert!(validate_len(&"n".repeat(65),64,"name").is_err());
    assert!(validate_len("",64,"name").is_err());
    assert_eq!(sanitize_single_line(" \n\t "), "");
    assert_eq!(sanitize_single_line("a\n  b"), "a b");
}
'''
    with tempfile.TemporaryDirectory(prefix="s5-meta-", dir=os.environ.get("TMPDIR")) as directory:
        root = Path(directory).resolve()
        for negative in (False, True):
            text = program.replace("if value.chars().count() > max_len", "if false") if negative else program
            path = root / "probe.rs"
            path.write_text(text)
            binary = root / "probe"
            subprocess.run(["rustc", "--edition=2024", "-Awarnings", str(path), "-o", str(binary)],
                           check=True, capture_output=True)
            result = subprocess.run([str(binary)], capture_output=True)
            assert (result.returncode != 0) == negative, "metadata function negative control failed"
    print(f"Pinned Codex metadata functions: 64/65, blank description, normalization and removed-limit negative control passed; SHA256 {SHA256}")
    print("Evidence is exact extracted native functions, not full YAML/native discovery or session loading.")


if __name__ == "__main__":
    main()
