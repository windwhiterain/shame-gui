"""Project cleanup pipeline: cargo check + cargo fix + cargo fmt.

Note: this script MODIFIES the tree — `cargo fix` rewrites sources to
apply compiler suggestions and `cargo fmt` reformats. Run before
committing, and review the diff afterwards.

Usage: python clean.py
"""

import subprocess
import sys

COMMANDS = [
    ["cargo", "check", "--workspace", "--all-targets"],
    ["cargo", "fix", "--allow-dirty", "--all-targets"],
    ["cargo", "fmt"],
]

for command in COMMANDS:
    print(f"> {' '.join(command)}")
    result = subprocess.run(command)
    if result.returncode != 0:
        print(f"FAILED: {' '.join(command)}")
        sys.exit(result.returncode)

print("clean.py: all checks passed.")
