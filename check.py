"""Project verification: cargo check + cargo fix + cargo fmt.

Usage: python check.py
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

print("check.py: all checks passed.")
