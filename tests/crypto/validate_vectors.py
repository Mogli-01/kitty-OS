"""Validate the format and dimensions of the official crypto fixtures."""

import hashlib
import json
from pathlib import Path
import re
import sys

VECTORS = Path(__file__).with_name("vectors")
LENGTHS = {
    "chacha20-poly1305": {"key": 32, "nonce": 12, "aad": 12, "plaintext": 114, "ciphertext": 114, "tag": 16},
    "ed25519": {"public_key": 32, "message": 0, "signature": 64},
    "sha256": {"message": 3, "digest": 32},
    "argon2id": {"password": 32, "salt": 16, "secret": 8, "associated_data": 12, "tag": 32},
}
ARGON2_PARAMETERS = {"version": 19, "memory_kib": 32, "passes": 3, "parallelism": 4}


def validate(name, lengths):
    data = json.loads((VECTORS / f"{name}.json").read_text(encoding="utf-8"))
    params = ARGON2_PARAMETERS if name == "argon2id" else {}
    if not isinstance(data, dict) or set(data) != set(lengths) | set(params) | {"source", "section"}:
        raise ValueError(f"{name}: unexpected fixture fields")
    if not isinstance(data["source"], str) or not data["source"].startswith("https://"):
        raise ValueError(f"{name}: missing source")
    if not isinstance(data["section"], str) or not data["section"]:
        raise ValueError(f"{name}: missing source section")
    for field, size in lengths.items():
        value = data[field]
        if not isinstance(value, str) or len(value) != 2 * size or not re.fullmatch("[0-9a-f]*", value):
            raise ValueError(f"{name}.{field}: expected {size} bytes of lowercase hex")
    for field, expected in params.items():
        if type(data[field]) is not int or data[field] != expected:
            raise ValueError(f"{name}.{field}: expected {expected}")
    if name == "sha256" and hashlib.sha256(bytes.fromhex(data["message"])).hexdigest() != data["digest"]:
        raise ValueError("sha256: message and reference digest differ")


if __name__ == "__main__":
    try:
        for name, lengths in LENGTHS.items():
            validate(name, lengths)
    except (OSError, ValueError) as error:
        sys.exit(f"invalid crypto fixture: {error}")
    print(f"Validated {len(LENGTHS)} official crypto fixtures")
