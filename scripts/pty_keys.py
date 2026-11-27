ENTER = b"\r"
ESC = b"\x1b"
DOWN = b"\x1b[B"
PAGE_UP = b"\x1b[5~"
PAGE_DOWN = b"\x1b[6~"


def install_form(source, kind=b"", ref=b"", skill=b"", agent=b"", scope=b""):
    fields = [str(source).encode(), kind, ref, skill, agent, scope]
    return b"".join(field + ENTER for field in fields)
