#!/usr/bin/env python3
"""
publish.py — Pin shame git deps and release shame-gui (or shame).

Workflow:
  1. In the shame repo (../shame), ensure HEAD is tagged (or use --shame-rev).
  2. In shame-gui, update Cargo.toml to pin shame deps to a specific tag/rev.
  3. Commit the pin, tag shame-gui, and push.
  4. After release, use --dev to reset Cargo.toml back to branch = "main".

Usage:
  python publish.py <version>                  # release shame-gui
  python publish.py <version> --squash -y      # squash all commits + release shame-gui
  python publish.py --shame <version> --squash -y  # squash + release shame
  python publish.py <version> --shame-rev <r>  # pin shame to a specific rev (or tag)
  python publish.py --dev                      # reset Cargo.toml to branch = "main"
  python publish.py <version> --dry-run        # show what would happen, don't execute
  python publish.py <version> --no-push        # don't push to remote
  python publish.py <version> --no-commit      # only update Cargo.toml, don't commit/tag

Examples:
  python publish.py --shame v2.0.0-beta.3 --squash -y   # release shame, squash 62 commits
  python publish.py v0.2.0 --squash -y                  # release shame-gui, squash 29 commits
  python publish.py v0.2.0 --shame-rev v2.0.0-beta.3
  python publish.py --dev                               # after release, switch back to branch deps
"""

import argparse
import subprocess
import sys
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parent
SHAME_DIR = ROOT / ".." / "shame"
CARGO_TOML = ROOT / "Cargo.toml"
SHAME_GIT_URL = "ssh://git@github.com/windwhiterain/shame.git"

SHAME_CRATES = ["shame", "shame_wgpu", "shame_derive"]


def run(cmd: list[str], cwd: Path, check: bool = True) -> subprocess.CompletedProcess:
    """Run a command, return CompletedProcess. Raises on failure if check=True."""
    try:
        return subprocess.run(
            cmd, cwd=str(cwd), capture_output=True, text=True, check=check
        )
    except subprocess.CalledProcessError as e:
        print(f"  ERROR: {' '.join(cmd)}")
        print(f"  stdout: {e.stdout}")
        print(f"  stderr: {e.stderr}")
        raise


def git(args: list[str], cwd: Path) -> str:
    """Run a git command and return stripped stdout."""
    return run(["git"] + args, cwd=cwd).stdout.strip()


def shame_head_tag() -> str | None:
    """Return the tag at shame HEAD, or None."""
    return git(["tag", "--points-at", "HEAD"], cwd=SHAME_DIR) or None


def shame_latest_tag() -> str | None:
    """Return the most recent tag in the shame repo."""
    result = git(["tag", "--sort=-creatordate"], cwd=SHAME_DIR)
    return result.split("\n")[0] if result else None


def cargo_toml_content() -> str:
    return CARGO_TOML.read_text(encoding="utf-8")


def update_cargo_toml(rev: str) -> str:
    """
    Replace branch = "main" (or an existing tag/rev) with the given rev.
    Returns the new content.
    """
    content = cargo_toml_content()

    # Pattern 1: shame* = { git = "...", branch = "main" }
    pattern_branch = re.compile(
        r'((' + "|".join(SHAME_CRATES) + r')\s*=\s*\{\s*git\s*=\s*"'
        + re.escape(SHAME_GIT_URL)
        + r'")\s*,\s*branch\s*=\s*"main"\s*\}'
    )

    # Pattern 2: shame* = { git = "...", (tag|rev) = "..." }  (already pinned)
    pattern_pinned = re.compile(
        r'((' + "|".join(SHAME_CRATES) + r')\s*=\s*\{\s*git\s*=\s*"'
        + re.escape(SHAME_GIT_URL)
        + r'")\s*,\s*(?:tag|rev)\s*=\s*"[^"]*"\s*\}'
    )

    if rev.startswith("v") and len(rev) > 1 and rev[1].isdigit():
        replacement = rf'\1, tag = "{rev}"}}'
    else:
        replacement = rf'\1, rev = "{rev}"}}'

    if pattern_branch.search(content):
        return pattern_branch.sub(replacement, content)
    if pattern_pinned.search(content):
        return pattern_pinned.sub(replacement, content)
    return content


def reset_cargo_toml_to_branch() -> str:
    """
    Replace any tag = "..." or rev = "..." with branch = "main".
    Returns the new content.
    """
    content = cargo_toml_content()

    # Pattern: shame* = { git = "...", (tag|rev) = "..." }
    pattern = re.compile(
        r'((' + "|".join(SHAME_CRATES) + r')\s*=\s*\{\s*git\s*=\s*"'
        + re.escape(SHAME_GIT_URL)
        + r'")\s*,\s*(?:tag|rev)\s*=\s*"[^"]*"\s*\}'
    )

    replacement = rf'\1, branch = "main"}}'
    return pattern.sub(replacement, content)


def current_branch(cwd: Path) -> str:
    return git(["rev-parse", "--abbrev-ref", "HEAD"], cwd=cwd)


def has_uncommitted(cwd: Path) -> bool:
    return bool(git(["status", "--porcelain"], cwd=cwd))


def git_root_commit(cwd: Path) -> str:
    """Return the root (first) commit of the current branch."""
    return git(["rev-list", "--max-parents=0", "HEAD"], cwd=cwd)


def git_commit_count_since(cwd: Path, base: str) -> int:
    """Return number of commits from base (exclusive) to HEAD."""
    return int(git(["rev-list", "--count", f"{base}..HEAD"], cwd=cwd))


def do_squash(cwd: Path, dry_run: bool, yes_flag: bool, label: str) -> bool:
    """
    Soft-reset to the root commit, squashing all commits into staging.
    Returns True if a squash was performed; False if nothing to squash.
    """
    root = git_root_commit(cwd)
    count = git_commit_count_since(cwd, root)
    if count == 0:
        print(f"[{label}] Only one commit on branch; nothing to squash.")
        return False

    print(f"[{label}] Will squash {count} commit(s) into one.")
    print(f"[{label}] Root commit: {root[:7]}")

    if dry_run:
        print(f"[{label}] (dry-run) would soft-reset to root, then commit everything.")
        return True

    if not yes_flag:
        resp = input(
            f"[{label}] This rewrites history. Continue? [y/N] "
        ).strip().lower()
        if resp not in ("y", "yes"):
            print(f"[{label}] Aborted.")
            sys.exit(0)

    git(["reset", "--soft", root], cwd=cwd)
    print(f"[{label}] Soft-reset to {root[:7]}; {count} commit(s) squashed into staging.")
    return True


def main():
    parser = argparse.ArgumentParser(
        description="Pin shame git deps and release shame-gui."
    )
    parser.add_argument(
        "version", nargs="?", help="shame-gui version tag, e.g. v0.2.0"
    )
    parser.add_argument(
        "--shame",
        action="store_true",
        help="Release the shame repo (../shame) instead of shame-gui.",
    )
    parser.add_argument(
        "--shame-rev",
        dest="shame_rev",
        help="Pin shame to this tag or rev (default: auto-detect tag at shame HEAD)",
    )
    parser.add_argument(
        "--dev",
        action="store_true",
        help="Reset Cargo.toml back to branch = \"main\" after a release.",
    )
    parser.add_argument(
        "--dry-run", action="store_true", help="Print what would happen, don't execute"
    )
    parser.add_argument(
        "--no-push", action="store_true", help="Don't push to remote"
    )
    parser.add_argument(
        "--no-commit",
        action="store_true",
        help="Only update Cargo.toml, don't commit or tag",
    )
    parser.add_argument(
        "--squash",
        action="store_true",
        help="Squash all commits on this branch into one clean release commit.",
    )
    parser.add_argument(
        "-y", "--yes",
        action="store_true",
        help="Skip confirmation prompts (for --squash).",
    )
    args = parser.parse_args()

    # ── --shame mode: release the shame repo ─────────────────────────────
    if args.shame:
        if not args.version:
            parser.error("version is required with --shame")
        version = args.version

        if args.squash:
            did_squash = do_squash(SHAME_DIR, args.dry_run, args.yes, "shame")
        else:
            did_squash = False

        if args.dry_run:
            print(f"\n── DRY RUN: shame {version} ──")
            if did_squash:
                print(f"[shame] would commit: release: {version}")
            print(f"[shame] would tag: {version}")
            if args.squash:
                branch = current_branch(SHAME_DIR)
                print(f"[shame] would force-push branch '{branch}' to origin")
            print(f"[shame] would push tag '{version}' to origin")
            return

        if args.no_commit:
            print("[shame] --no-commit: stopping before commit/tag.")
            return

        if did_squash:
            # After soft reset, everything is staged; commit as the release
            git(["add", "-A"], cwd=SHAME_DIR)
            git(["commit", "-m", f"release: {version}"], cwd=SHAME_DIR)
            print(f"[shame] Committed: release: {version}")

        git(["tag", "-a", "-f", version, "-m", f"Release {version}"], cwd=SHAME_DIR)
        print(f"[shame] Tagged: {version}")

        if args.no_push:
            print("[shame] --no-push: commit + tag created locally, stopping.")
            return

        branch = current_branch(SHAME_DIR)
        if did_squash:
            git(["push", "--force-with-lease", "origin", branch], cwd=SHAME_DIR)
            print(f"[shame] Force-pushed squashed branch '{branch}' to origin.")
        else:
            git(["push", "origin", branch], cwd=SHAME_DIR)
            print(f"[shame] Pushed branch '{branch}' to origin.")
        git(["push", "origin", version], cwd=SHAME_DIR)
        print(f"[shame] Pushed tag '{version}' to origin.")

        print(f"\n✓ Shame {version} release complete!")
        return

    # ── --dev mode: reset to branch = "main" ─────────────────────────────
    if args.dev:
        new_cargo = reset_cargo_toml_to_branch()
        old_cargo = cargo_toml_content()
        if new_cargo == old_cargo:
            print("[shame-gui] Cargo.toml already uses branch = \"main\" — nothing to change.")
            return
        if args.dry_run:
            print("\n── DRY RUN ──")
            print("[shame-gui] would reset Cargo.toml: pinned → branch = \"main\"")
            return
        CARGO_TOML.write_text(new_cargo, encoding="utf-8")
        print("[shame-gui] Cargo.toml reset to branch = \"main\".")
        return

    # ── release mode needs a version ─────────────────────────────────────
    if not args.version:
        parser.error("version is required for release (or use --dev to reset)")
    version = args.version
    if not re.match(r"^v\d+\.\d+\.\d+", version):
        print(f"WARNING: version '{version}' doesn't look like semver (vX.Y.Z)")

    # ── 1. Determine shame rev ──────────────────────────────────────────
    shame_rev = args.shame_rev
    if not shame_rev:
        shame_rev = shame_head_tag()
        if shame_rev:
            print(f"[shame] HEAD is tagged: {shame_rev}")
        elif args.dry_run:
            shame_rev = git(["rev-parse", "--short=7", "HEAD"], cwd=SHAME_DIR)
            print(f"[shame] HEAD is NOT tagged; would use rev: {shame_rev}")
        else:
            latest = shame_latest_tag() or "(none)"
            print(f"[shame] HEAD is NOT tagged (latest tag: {latest})")
            print("[shame] Pass --shame-rev to pin to a specific tag/rev, or")
            resp = input("[shame] Create a new tag now? Enter tag name (or skip): ").strip()
            if resp:
                shame_rev = resp
                git(["tag", shame_rev], cwd=SHAME_DIR)
                print(f"[shame] Created tag: {shame_rev}")
            else:
                # fall back to commit hash
                shame_rev = git(["rev-parse", "--short=7", "HEAD"], cwd=SHAME_DIR)
                print(f"[shame] Using raw rev: {shame_rev}")

    # ── 1.5 Squash (optional) ──────────────────────────────────────────
    did_squash = False
    if args.squash:
        did_squash = do_squash(ROOT, args.dry_run, args.yes, "shame-gui")

    # ── 2. Update Cargo.toml ────────────────────────────────────────────
    new_cargo = update_cargo_toml(shame_rev)
    old_cargo = cargo_toml_content()

    if new_cargo == old_cargo:
        print("[shame-gui] Cargo.toml already up to date — nothing to change.")
        return

    if args.dry_run:
        print("\n── DRY RUN ──")
        print(f"[shame] rev to pin: {shame_rev}")
        print(f"[shame-gui] would update Cargo.toml:")
        print(f"[shame-gui] would update Cargo.toml to pin shame @ {shame_rev}")
        print(f"[shame-gui] would commit + tag: {version}")
        if not args.no_push:
            print(f"[shame-gui] would push to origin/{current_branch(ROOT)}")
        return

    CARGO_TOML.write_text(new_cargo, encoding="utf-8")
    print(f"[shame-gui] Cargo.toml updated: shame deps pinned to {shame_rev}")

    if args.no_commit:
        print("[shame-gui] --no-commit: Cargo.toml updated, stopping.")
        return

    # ── 3. Commit + tag + push ──────────────────────────────────────────
    if not did_squash and has_uncommitted(ROOT):
        # check if only Cargo.toml changed
        status = git(["status", "--porcelain"], cwd=ROOT)
        if status.strip() == f"M {CARGO_TOML.name}":
            print(f"[shame-gui] Cargo.toml was already modified; committing existing change.")
        else:
            print("[shame-gui] WARNING: uncommitted changes besides Cargo.toml:")
            print(status)
            resp = input("[shame-gui] Continue? [y/N] ").strip().lower()
            if resp not in ("y", "yes"):
                print("[shame-gui] Aborted.")
                return

    if did_squash:
        # Stage everything (soft-reset left all old changes staged; Cargo.toml is new)
        git(["add", "-A"], cwd=ROOT)
        commit_msg = f"release: {version}"
    else:
        git(["add", "Cargo.toml"], cwd=ROOT)
        commit_msg = f"release: {version} (pin shame to {shame_rev})"

    git(["commit", "-m", commit_msg], cwd=ROOT)
    print(f"[shame-gui] Committed: {commit_msg}")

    git(["tag", "-a", "-f", version, "-m", f"Release {version}"], cwd=ROOT)
    print(f"[shame-gui] Tagged: {version}")

    if args.no_push:
        print("[shame-gui] --no-push: commit + tag created locally, stopping.")
        return

    branch = current_branch(ROOT)
    if did_squash:
        git(["push", "--force-with-lease", "origin", branch], cwd=ROOT)
        print(f"[shame-gui] Force-pushed squashed branch '{branch}' to origin.")
    else:
        git(["push", "origin", branch], cwd=ROOT)
        print(f"[shame-gui] Pushed branch '{branch}' and tag '{version}' to origin.")
    git(["push", "origin", version], cwd=ROOT)

    # If we created a new shame tag, push it too
    if not args.shame_rev and shame_rev:
        existing = git(["tag", "--points-at", "HEAD"], cwd=SHAME_DIR)
        if shame_rev in existing:
            print(f"[shame] Pushing new tag '{shame_rev}'...")
            git(["push", "origin", shame_rev], cwd=SHAME_DIR)

    print(f"\n✓ Release {version} complete!")


if __name__ == "__main__":
    main()
