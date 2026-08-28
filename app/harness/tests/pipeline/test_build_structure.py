import tempfile
import unittest
from pathlib import Path

from errors import StageFailedError
from execution.paths import HarnessPaths
from pipeline.build_structure import (
    discover_legacy_build_files,
    run_build_structure_agent,
)


class FakeAgent:
    def __init__(
        self,
        write_root_build_file: bool = True,
        write_report: bool = True,
    ) -> None:
        self.write_root_build_file = write_root_build_file
        self.write_report = write_report
        self.calls = []

    def run_build_structure(
        self,
        rewrite_workspace,
        legacy_repo_path,
        build_report_path,
        compatibility_report_path,
        legacy_tree_path,
        dependency_selection_report_path,
        build_structure_report_path,
        target_version,
        allowed_legacy_build_files,
    ):
        self.calls.append(
            (
                rewrite_workspace,
                legacy_repo_path,
                build_report_path,
                compatibility_report_path,
                legacy_tree_path,
                dependency_selection_report_path,
                build_structure_report_path,
                target_version,
                allowed_legacy_build_files,
            )
        )
        rewrite_workspace.mkdir(parents=True, exist_ok=True)
        if self.write_root_build_file:
            (rewrite_workspace / "pom.xml").write_text("<project />\n", encoding="utf-8")
        if self.write_report:
            build_structure_report_path.write_text("# Build Structure\n", encoding="utf-8")


class BuildStructureTests(unittest.TestCase):
    def test_discovers_only_legacy_build_files(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repo = Path(directory) / "repo"
            (repo / "service" / "src" / "main" / "java").mkdir(parents=True)
            (repo / ".mvn" / "wrapper").mkdir(parents=True)
            (repo / "gradle" / "wrapper").mkdir(parents=True)
            (repo / "pom.xml").write_text("<project />\n", encoding="utf-8")
            (repo / "service" / "pom.xml").write_text("<project />\n", encoding="utf-8")
            (repo / ".mvn" / "wrapper" / "maven-wrapper.properties").write_text(
                "distributionUrl=x\n",
                encoding="utf-8",
            )
            (repo / "gradle" / "wrapper" / "gradle-wrapper.properties").write_text(
                "distributionUrl=x\n",
                encoding="utf-8",
            )
            (repo / "service" / "src" / "main" / "java" / "App.java").write_text(
                "class App {}\n",
                encoding="utf-8",
            )

            files = discover_legacy_build_files(repo)

        self.assertEqual(
            files,
            [
                repo / ".mvn" / "wrapper" / "maven-wrapper.properties",
                repo / "gradle" / "wrapper" / "gradle-wrapper.properties",
                repo / "pom.xml",
                repo / "service" / "pom.xml",
            ],
        )

    def test_runs_agent_and_requires_root_build_file_and_report(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            paths = HarnessPaths.from_org_project("org/project", Path(directory))
            (paths.repo / "service").mkdir(parents=True)
            (paths.repo / "pom.xml").write_text("<project />\n", encoding="utf-8")
            (paths.repo / "service" / "pom.xml").write_text(
                "<project />\n",
                encoding="utf-8",
            )
            agent = FakeAgent()

            run_build_structure_agent(paths, agent, "25")

            self.assertEqual(len(agent.calls), 1)
            call = agent.calls[0]
            self.assertEqual(call[0], paths.rewrite_workspace)
            self.assertEqual(call[1], paths.repo)
            self.assertEqual(call[2], paths.build_report)
            self.assertEqual(call[3], paths.compatibility_report)
            self.assertEqual(call[4], paths.legacy_tree)
            self.assertEqual(call[5], paths.dependency_selection_report)
            self.assertEqual(call[6], paths.build_structure_report)
            self.assertEqual(call[7], "25")
            self.assertEqual(
                call[8],
                [paths.repo / "pom.xml", paths.repo / "service" / "pom.xml"],
            )

    def test_fails_when_agent_does_not_write_root_build_file(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            paths = HarnessPaths.from_org_project("org/project", Path(directory))
            paths.repo.mkdir(parents=True)

            with self.assertRaises(StageFailedError):
                run_build_structure_agent(
                    paths,
                    FakeAgent(write_root_build_file=False),
                    "25",
                )

    def test_fails_when_agent_does_not_write_report(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            paths = HarnessPaths.from_org_project("org/project", Path(directory))
            paths.repo.mkdir(parents=True)

            with self.assertRaises(StageFailedError):
                run_build_structure_agent(paths, FakeAgent(write_report=False), "25")


if __name__ == "__main__":
    unittest.main()
