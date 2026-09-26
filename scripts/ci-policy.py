#!/usr/bin/env python3
"""Select docs-only CI conservatively; use only the Python standard library."""

import argparse
import json
import os
import subprocess  # nosec B404
from pathlib import Path

# These documents are also application, test, or packaging inputs.
CODE_DOCUMENTS = frozenset(['docs/extension-tutorial.md'])
DOC_MARKDOWN_DIRS = ("docs/",)
DOC_FILES = frozenset(
    {
        "zensical.toml",
        "mkdocs.yml",
        ".github/docs-tools.env",
        "scripts/docs.sh",
        ".github/workflows/docs.yml",
        ".markdownlint.json",
    }
)


def classify(paths: list[str]) -> dict[str, bool]:
    def documentation(path: str) -> bool:
        if path in CODE_DOCUMENTS:
            return False
        return (
            path in DOC_FILES
            or (
                path.endswith(".md")
                and ("/" not in path or path.startswith(DOC_MARKDOWN_DIRS))
            )
            or path.startswith("docs/assets/")
        )

    return {
        "code": not paths or any(not documentation(path) for path in paths),
        "markdown": any(
            path.endswith(".md") or path == ".markdownlint.json" for path in paths
        ),
    }


def changed_paths(root: Path, event_name: str, event: dict) -> list[str] | None:
    """None means no safe diff is available, so all checks must run."""
    if event_name == "pull_request":
        pull = event["pull_request"]
        if any(label["name"] == "ci:full" for label in pull.get("labels", [])):
            return None
        base, head = pull["base"]["sha"], pull["head"]["sha"]
        if base and head:
            base = subprocess.check_output(  # nosec B603, B607
                ["git", "merge-base", base, head], cwd=root, text=True
            ).strip()
    elif event_name == "push":
        if event.get("ref", "").startswith("refs/tags/"):
            return None
        base, head = event.get("before"), event.get("after")
    elif event_name == "merge_group":
        group = event["merge_group"]
        base, head = group["base_sha"], group["head_sha"]
    else:
        # Manual, scheduled, and reusable release CI retain complete validation.
        return None
    if not base or not head or set(base) == {"0"}:
        return None
    output = subprocess.check_output(  # nosec B603, B607
        ["git", "diff", "--no-renames", "--name-only", "-z", base, head, "--"], cwd=root
    )
    return [os.fsdecode(path) for path in output.split(b"\0") if path]


def check_gate(jobs: dict, code_jobs: list[str]) -> None:
    changes = jobs["changes"]
    if changes["result"] != "success":
        raise ValueError("CI classification did not succeed")
    outputs = changes["outputs"]
    for flag in ("code", "markdown"):
        if outputs.get(flag) not in ("true", "false"):
            raise ValueError(f"Missing or invalid classification: {flag}")
    required = ["changes"]
    if outputs["code"] == "true":
        required.extend(code_jobs)
    if outputs["markdown"] == "true":
        required.append("markdown")
    for name, job in jobs.items():
        if job["result"] not in ("success", "skipped"):
            raise ValueError(f"{name} did not pass: {job['result']}")
    for name in required:
        if jobs[name]["result"] != "success":
            raise ValueError(f"Required job {name} did not run successfully")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--gate", action="store_true")
    args = parser.parse_args()
    if args.gate:
        check_gate(
            json.loads(os.environ["JOB_RESULTS"]), os.environ["CODE_JOBS"].split()
        )
        return
    root = Path(__file__).resolve().parents[1]
    event = json.loads(Path(os.environ["GITHUB_EVENT_PATH"]).read_text())
    paths = changed_paths(root, os.environ["GITHUB_EVENT_NAME"], event)
    flags = {"code": True, "markdown": True} if paths is None else classify(paths)
    output = "".join(f"{key}={str(value).lower()}\n" for key, value in flags.items())
    print(output, end="")
    with Path(os.environ["GITHUB_OUTPUT"]).open("a") as destination:
        destination.write(output)


if __name__ == "__main__":
    main()
