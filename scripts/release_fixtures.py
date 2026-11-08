"""Minimal executable headers for archive tests; the bytes are never run."""
import struct

ELF_MACHINE = {"x86_64-unknown-linux-gnu": 62, "aarch64-unknown-linux-gnu": 183}
MACHO_CPU = {"x86_64-apple-darwin": 0x01000007, "aarch64-apple-darwin": 0x0100000C}


def stub_binary(target, payload=b"stub"):
    """Smallest 64-bit little-endian ELF or Mach-O header for the target."""
    if target in ELF_MACHINE:
        header = b"\x7fELF" + bytes([2, 1, 1]) + bytes(9) + struct.pack("<HH", 3, ELF_MACHINE[target])
        header += bytes(64 - len(header))
    else:
        header = struct.pack("<8I", 0xFEEDFACF, MACHO_CPU[target], 0, 2, 0, 0, 0, 0)
    return header + payload
