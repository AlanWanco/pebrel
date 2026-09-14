from __future__ import annotations

import subprocess
from pathlib import Path
import tomllib
import unittest
from unittest.mock import patch

from scripts.ci_native_tests import main, native_commands


class NativeSuiteTests(unittest.TestCase):
    def test_full_workspace_and_interactions_share_one_unfiltered_invocation(self):
        rust = [command for command in native_commands() if command[:2] == ["cargo", "test"]]
        self.assertEqual(len(rust), 1)
        command = rust[0]
        self.assertEqual(command[1], "test")
        self.assertIn("--locked", command)
        self.assertIn("--workspace", command)
        self.assertEqual(command[command.index("--features") + 1], "nebula/gpui-test-support")
        self.assertNotIn("--exclude", command)
        self.assertNotIn("--lib", command)
        self.assertNotIn("--skip", command)
        self.assertNotIn("--", command)

    def test_actual_product_feature_graph_is_also_checked(self):
        checks = [command for command in native_commands() if command[:2] == ["cargo", "check"]]
        self.assertEqual(len(checks), 1)
        command = checks[0]
        self.assertEqual(command[command.index("--features") + 1], "gpui-shell")
        self.assertEqual(command[command.index("--bin") + 1], "pebrel")

    def test_fast_test_profile_preserves_runtime_checks_and_resets_named_overrides(self):
        root = Path(__file__).resolve().parents[2]
        config = tomllib.loads((root / ".github/ci-profile.toml").read_text())
        workspace = tomllib.loads((root / "Cargo.toml").read_text())
        profile = config["profile"]["ci"]
        self.assertEqual(profile["inherits"], "dev")
        self.assertTrue(profile["debug-assertions"])
        self.assertTrue(profile["overflow-checks"])
        for package in workspace["profile"]["dev"]["package"]:
            self.assertEqual(profile["package"][package]["opt-level"], 0)
        self.assertNotIn("release", config["profile"])

    def test_both_python_test_roots_are_discovered(self):
        suites = [command for command in native_commands() if "unittest" in command]
        self.assertEqual(
            {command[command.index("-s") + 1] for command in suites},
            {"scripts/tests", "scripts/conformance/tests"},
        )
        self.assertTrue(all("discover" in command for command in suites))

    def test_success_runs_every_command_and_checks_exit_codes(self):
        with patch("scripts.ci_native_tests.subprocess.run") as run:
            self.assertEqual(main(), 0)
        self.assertEqual(run.call_count, len(native_commands()))
        self.assertTrue(all(call.kwargs["check"] for call in run.call_args_list))

    def test_failure_stops_the_suite_and_is_not_reported_as_success(self):
        with patch("scripts.ci_native_tests.subprocess.run") as run:
            run.side_effect = subprocess.CalledProcessError(7, "test")
            with self.assertRaises(subprocess.CalledProcessError):
                main()
        self.assertEqual(run.call_count, 1)


if __name__ == "__main__":
    unittest.main()
