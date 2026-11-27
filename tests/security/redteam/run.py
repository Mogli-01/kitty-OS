"""Run reviewed QEMU protection checks and validate their issue register."""

import argparse
import json
from pathlib import Path
import subprocess


ROOT = Path(__file__).resolve().parents[3]
MANIFEST = Path(__file__).with_name("manifest.json")
CHECKS = {
    "boot-page-protections": (
        "cargo", "xtask", "test", "--release",
        "--expect", "[mm] W^X audit:",
        "--expect", "0 writable+executable, 0 user pages in kernel half",
    ),
}


def load_checks():
    manifest = json.loads(MANIFEST.read_text(encoding="utf-8"))
    if not isinstance(manifest, dict) or set(manifest) != {"checks", "open_issues"}:
        raise ValueError("manifest must contain checks and open_issues")
    checks = manifest["checks"]
    if not isinstance(checks, list) or not checks:
        raise ValueError("checks must be a nonempty list")
    if any(not isinstance(check, str) or check not in CHECKS for check in checks):
        raise ValueError("checks must reference registered protection tests")
    if len(set(checks)) != len(checks) or set(checks) != CHECKS.keys():
        raise ValueError("manifest must list each registered test once")

    issues = manifest["open_issues"]
    if not isinstance(issues, list):
        raise ValueError("open_issues must be a list")
    seen = set()
    for entry in issues:
        if not isinstance(entry, dict) or set(entry) != {"check", "issue"}:
            raise ValueError("issue entries require check and issue")
        check, issue = entry["check"], entry["issue"]
        if not isinstance(check, str) or check not in checks or type(issue) is not int or issue < 1:
            raise ValueError("issue entries must name a check and a positive issue number")
        if (check, issue) in seen:
            raise ValueError("duplicate issue entry")
        seen.add((check, issue))
        print(f"{check}: issue #{issue} aberta", flush=True)
    return checks


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--validate-only", action="store_true")
    args = parser.parse_args()
    try:
        checks = load_checks()
    except (OSError, ValueError) as error:
        parser.exit(1, f"invalid security manifest: {error}\n")
    if not args.validate_only:
        failed = []
        for check in checks:
            try:
                subprocess.run(CHECKS[check], cwd=ROOT, check=True, timeout=180)
            except (OSError, subprocess.SubprocessError) as error:
                failed.append(check)
                print(f"{check}: FALHA ({error})", flush=True)
            else:
                print(f"{check}: PROTEÇÃO VERIFICADA", flush=True)
        if failed:
            parser.exit(1, "security checks failed: " + ", ".join(failed) + "\n")


if __name__ == "__main__":
    main()
