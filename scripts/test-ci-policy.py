#!/usr/bin/env python3
"""Regression tests for docs-only selection and failure propagation."""

# CI policy tests intentionally use unittest so the classifier needs no dependencies.
# ruff: noqa: PT009, PT027

import importlib.util
import re
import subprocess  # nosec B404
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location(
    "ci_policy", ROOT / "scripts/ci-policy.py"
)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError("Cannot load CI policy")
sys.dont_write_bytecode = True
policy = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(policy)


class ClassificationTests(unittest.TestCase):
    def test_prose_and_site_inputs_skip_application(self):
        for path in (
            "README.md",
            "AGENTS.md",
            "docs/new-page.md",
            "docs/old page.md",
            "docs/assets/stylesheets/extra.css",
            "zensical.toml",
            ".github/docs-tools.env",
            ".github/workflows/docs.yml",
        ):
            with self.subTest(path=path):
                self.assertFalse(policy.classify([path])["code"])

    def test_unknown_and_executable_inputs_select_application(self):
        for path in (
            "src/main.rs",
            "src/notes.md",
            "tests/fixture.md",
            "docs/openapi.json",
            "docs/example.py",
            "Cargo.lock",
            "package.json",
            "pyproject.toml",
            ".github/workflows/ci.yml",
            "scripts/ci-policy.py",
            "new-build-input",
        ):
            with self.subTest(path=path):
                self.assertTrue(policy.classify([path])["code"])

    def test_declared_code_documents_select_application(self):
        for path in policy.CODE_DOCUMENTS:
            with self.subTest(path=path):
                self.assertTrue(policy.classify([path])["code"])

    def test_mixed_changes_select_application(self):
        self.assertTrue(policy.classify(["docs/new-page.md", "src/main.rs"])["code"])

    def test_empty_diff_is_conservative(self):
        self.assertTrue(policy.classify([])["code"])

    def test_markdown_lint_selection(self):
        for path, expected in (
            ("README.md", True),
            (".markdownlint.json", True),
            ("src/main.rs", False),
        ):
            with self.subTest(path=path):
                self.assertEqual(policy.classify([path])["markdown"], expected)

    def test_literal_rust_includes_keep_code_validation(self):
        # Discover multiline normal and raw string literals without third-party tooling.
        pattern = re.compile(
            r'include_(?:str|bytes)!\s*\(\s*(?:r(#{0,})"(.*?)"\1|"([^"\n]+)")', re.S
        )
        tracked = subprocess.check_output(  # nosec B603, B607
            ["git", "ls-files", "-z", "*.rs"], cwd=ROOT
        )
        for entry in tracked.split(b"\0"):
            if not entry:
                continue
            source = ROOT / entry.decode()
            for match in pattern.finditer(source.read_text()):
                path = (source.parent / (match[2] or match[3])).resolve()
                if path.is_relative_to(ROOT):
                    relative = path.relative_to(ROOT).as_posix()
                    with self.subTest(source=source, included=relative):
                        self.assertTrue(policy.classify([relative])["code"])


class DiffTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.git("init", "-q", "-b", "main")
        self.git("config", "user.email", "ci@example.invalid")
        self.git("config", "user.name", "CI test")
        self.git("config", "commit.gpgSign", "false")
        (self.root / "README.md").write_text("initial\n")
        self.base = self.commit()

    def git(self, *args):
        return subprocess.check_output(  # nosec B603, B607
            ["git", *args], cwd=self.root, text=True
        ).strip()

    def commit(self):
        self.git("add", "--all")
        self.git("-c", "core.hooksPath=/dev/null", "commit", "-qm", "fixture")
        return self.git("rev-parse", "HEAD")

    def test_pull_request_uses_merge_base(self):
        self.git("checkout", "-qb", "docs")
        (self.root / "README.md").write_text("edited\n")
        head = self.commit()
        self.git("checkout", "-q", "main")
        (self.root / "application.rs").write_text("unrelated main change\n")
        base = self.commit()
        event = {"pull_request": {"base": {"sha": base}, "head": {"sha": head}}}
        self.assertEqual(
            policy.changed_paths(self.root, "pull_request", event), ["README.md"]
        )

    def test_rename_keeps_deleted_code_path(self):
        (self.root / "app.rs").write_text("code\n")
        base = self.commit()
        self.git("mv", "app.rs", "guide.md")
        head = self.commit()
        paths = policy.changed_paths(self.root, "push", {"before": base, "after": head})
        self.assertEqual(paths, ["app.rs", "guide.md"])
        self.assertTrue(policy.classify(paths)["code"])

    def test_deleted_document_is_docs_only(self):
        (self.root / "README.md").unlink()
        head = self.commit()
        paths = policy.changed_paths(
            self.root, "push", {"before": self.base, "after": head}
        )
        self.assertFalse(policy.classify(paths)["code"])

    def test_merge_group_uses_group_diff(self):
        (self.root / "README.md").write_text("edited\n")
        head = self.commit()
        event = {"merge_group": {"base_sha": self.base, "head_sha": head}}
        self.assertEqual(
            policy.changed_paths(self.root, "merge_group", event), ["README.md"]
        )

    def test_full_validation_events(self):
        for name, event in (
            ("workflow_dispatch", {}),
            ("workflow_call", {}),
            ("schedule", {}),
            ("push", {"before": "0" * 40, "after": self.base}),
            ("push", {"ref": "refs/tags/v1.0.0"}),
            ("pull_request", {"pull_request": {"labels": [{"name": "ci:full"}]}}),
        ):
            with self.subTest(event=name, payload=event):
                self.assertIsNone(policy.changed_paths(self.root, name, event))

    def test_failed_diff_cannot_report_docs_only(self):
        with self.assertRaises(subprocess.CalledProcessError):
            policy.changed_paths(
                self.root, "push", {"before": "missing", "after": self.base}
            )


class GateTests(unittest.TestCase):
    def jobs(self, code="false"):
        return {
            "changes": {
                "result": "success",
                "outputs": {"code": code, "markdown": "true"},
            },
            "markdown": {"result": "success"},
            "test": {"result": "skipped"},
        }

    def test_docs_only_accepts_intentionally_skipped_code(self):
        policy.check_gate(self.jobs(), ["test"])

    def test_code_requires_code_job_to_run(self):
        with self.assertRaises(ValueError):
            policy.check_gate(self.jobs("true"), ["test"])

    def test_failed_or_cancelled_job_fails_gate(self):
        for name in ("changes", "markdown", "test"):
            for result in ("failure", "cancelled"):
                with self.subTest(job=name, result=result):
                    jobs = self.jobs()
                    jobs[name]["result"] = result
                    with self.assertRaises(ValueError):
                        policy.check_gate(jobs, ["test"])

    def test_missing_classification_fails_gate(self):
        jobs = self.jobs()
        del jobs["changes"]["outputs"]["code"]
        with self.assertRaises(ValueError):
            policy.check_gate(jobs, ["test"])

    def test_skipped_markdown_fails_gate(self):
        jobs = self.jobs()
        jobs["markdown"]["result"] = "skipped"
        with self.assertRaises(ValueError):
            policy.check_gate(jobs, ["test"])


if __name__ == "__main__":
    unittest.main()
