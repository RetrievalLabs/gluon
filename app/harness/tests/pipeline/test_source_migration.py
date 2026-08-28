import tempfile
import unittest
from pathlib import Path

from errors import StageFailedError
from execution.paths import HarnessPaths
from pipeline.source_migration import run_source_migration_agent


class FakeAgent:
    def __init__(self, write_source: bool = True, write_report: bool = True) -> None:
        self.write_source = write_source
        self.write_report = write_report
        self.calls = []

    def run_source_migration(
        self,
        rewrite_workspace,
        legacy_repo_path,
        business_kg_db_path,
        extraction_db_path,
        characterization_db_path,
        characterization_output_dir,
        target_version,
        output_path,
    ):
        self.calls.append(
            (
                rewrite_workspace,
                legacy_repo_path,
                business_kg_db_path,
                extraction_db_path,
                characterization_db_path,
                characterization_output_dir,
                target_version,
                output_path,
            )
        )
        if self.write_source:
            source = rewrite_workspace / "src" / "main" / "java" / "App.java"
            source.parent.mkdir(parents=True, exist_ok=True)
            source.write_text("class App {}\n", encoding="utf-8")
        if self.write_report:
            output_path.write_text("# Source Migration\n", encoding="utf-8")


class SourceMigrationTests(unittest.TestCase):
    def test_runs_agent_and_requires_source_and_report(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            paths = HarnessPaths.from_org_project("org/project", Path(directory))
            agent = FakeAgent()

            run_source_migration_agent(paths, agent, "25")

            self.assertEqual(len(agent.calls), 1)
            call = agent.calls[0]
            self.assertEqual(call[0], paths.rewrite_workspace)
            self.assertEqual(call[1], paths.repo)
            self.assertEqual(call[2], paths.business_kg_db)
            self.assertEqual(call[3], paths.extraction_db)
            self.assertEqual(call[4], paths.characterization_db)
            self.assertEqual(call[5], paths.characterization_output_dir)
            self.assertEqual(call[6], "25")
            self.assertEqual(call[7], paths.source_migration_report)

    def test_fails_when_agent_does_not_write_source(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            paths = HarnessPaths.from_org_project("org/project", Path(directory))

            with self.assertRaises(StageFailedError):
                run_source_migration_agent(paths, FakeAgent(write_source=False), "25")

    def test_fails_when_agent_does_not_write_report(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            paths = HarnessPaths.from_org_project("org/project", Path(directory))

            with self.assertRaises(StageFailedError):
                run_source_migration_agent(paths, FakeAgent(write_report=False), "25")


if __name__ == "__main__":
    unittest.main()
