import tempfile
import unittest
from pathlib import Path

from errors import StageFailedError
from execution.paths import HarnessPaths
from pipeline.dependency_selection import run_dependency_selection_agent


class FakeAgent:
    def __init__(self, write_report: bool = True) -> None:
        self.write_report = write_report
        self.calls = []

    def run_dependency_selection(
        self,
        rewrite_workspace,
        legacy_repo_path,
        build_report_path,
        compatibility_report_path,
        target_version,
        output_path,
    ):
        self.calls.append(
            (
                rewrite_workspace,
                legacy_repo_path,
                build_report_path,
                compatibility_report_path,
                target_version,
                output_path,
            )
        )
        if self.write_report:
            output_path.write_text("# Dependency Selection\n", encoding="utf-8")


class DependencySelectionTests(unittest.TestCase):
    def test_runs_agent_and_requires_markdown_output(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            paths = HarnessPaths.from_org_project("org/project", Path(directory))
            agent = FakeAgent()

            run_dependency_selection_agent(paths, agent, "25")

            self.assertEqual(len(agent.calls), 1)
            call = agent.calls[0]
            self.assertEqual(call[0], paths.rewrite_workspace)
            self.assertEqual(call[1], paths.repo)
            self.assertEqual(call[2], paths.build_report)
            self.assertEqual(call[3], paths.compatibility_report)
            self.assertEqual(call[4], "25")
            self.assertEqual(call[5], paths.dependency_selection_report)
            self.assertEqual(
                paths.dependency_selection_report.read_text(encoding="utf-8"),
                "# Dependency Selection\n",
            )

    def test_fails_when_agent_does_not_write_report(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            paths = HarnessPaths.from_org_project("org/project", Path(directory))

            with self.assertRaises(StageFailedError):
                run_dependency_selection_agent(paths, FakeAgent(False), "25")


if __name__ == "__main__":
    unittest.main()
