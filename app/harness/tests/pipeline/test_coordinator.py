import os
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from execution.paths import HarnessPaths
from models.command import CommandResult
from models.config import HarnessConfig, RepoInfo
from pipeline.coordinator import PipelineCoordinator


class CoordinatorTests(unittest.TestCase):
    def test_runs_successful_stage_sequence(self) -> None:
        config = HarnessConfig(
            backend_url="mock://local",
            language="java",
            current_version="9",
            target_version="25",
            org_project_name="org/project",
            anthropic_api_key="key",
            anthropic_model="model",
            anthropic_base_url="base",
        )
        env = {"MOCK_REPO_URL": "https://repo.test/project", "MOCK_SOURCE_BRANCH": "main"}

        with tempfile.TemporaryDirectory() as directory:
            paths = HarnessPaths.from_org_project("org/project", Path(directory))

            with mock.patch(
                "pipeline.coordinator.HarnessPaths.from_org_project",
                return_value=paths,
            ), mock.patch(
                "pipeline.coordinator.BackendClient.fetch_repo",
                return_value=RepoInfo("repo", "main"),
            ), mock.patch(
                "pipeline.coordinator.ClaudeAgentClient.validate",
            ), mock.patch(
                "pipeline.coordinator.ClaudeAgentClient.close",
            ) as close_agent, mock.patch(
                "pipeline.coordinator.GitWorkspace.prepare",
            ), mock.patch(
                "pipeline.coordinator.run_migration_rewrite_setup",
            ), mock.patch(
                "execution.commands.CommandRunner.run",
                return_value=CommandResult(["cmd"], None, 0, "{}", "", 1),
            ):
                summary = PipelineCoordinator(config, env).run()

        self.assertEqual(summary.status, "ok")
        self.assertEqual(
            summary.completed_stages,
            [
                "parse-build",
                "analyze-report",
                "extract-business",
                "extract-tests",
                "build-business-kg",
                "generate-characterization-tests",
                "migration-rewrite",
            ],
        )
        self.assertEqual(close_agent.call_count, 2)


if __name__ == "__main__":
    unittest.main()
