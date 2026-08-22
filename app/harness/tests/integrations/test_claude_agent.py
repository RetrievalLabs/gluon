import json
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from integrations.claude_agent import ClaudeAgentClient
from models.command import CommandResult
from models.config import HarnessConfig


class ClaudeAgentTests(unittest.TestCase):
    def test_records_repair_attempt(self) -> None:
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
        failed = CommandResult(["cmd"], "/repo", 1, "", "failed", 1)
        with tempfile.TemporaryDirectory() as directory:
            log_path = Path(directory) / "agents.jsonl"

            with mock.patch.object(
                ClaudeAgentClient,
                "run_agent",
                return_value="fixed",
            ) as run_agent:
                attempt = ClaudeAgentClient(config, log_path).repair_stage(
                    "parse-build",
                    1,
                    Path("/repo"),
                    failed,
                )

            self.assertEqual(attempt.stage_name, "parse-build")
            self.assertEqual(attempt.status, "completed")
            self.assertIn("command `cmd`", attempt.message)
            self.assertIn("cwd `/repo`", attempt.message)
            run_agent.assert_called_once()
            prompt = run_agent.call_args.args[1]
            self.assertIn("Command: cmd", prompt)
            self.assertIn("Gluon CLI skill:", prompt)
            record = json.loads(log_path.read_text(encoding="utf-8"))
            self.assertEqual(record["attempt"], 1)
            self.assertIn("command `cmd`", record["message"])

    def test_agent_options_allow_repair_tools(self) -> None:
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

        options = ClaudeAgentClient(config).agent_options_kwargs(Path("/repo"))

        self.assertEqual(options["cwd"], Path("/repo"))
        self.assertEqual(options["model"], "model")
        self.assertIn("Bash", options["tools"])
        self.assertIn("Edit", options["allowed_tools"])
        self.assertEqual(options["permission_mode"], "dontAsk")
        self.assertIn("system_prompt", options)

    def test_gluon_cli_skill_path_can_be_configured(self) -> None:
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

        with tempfile.TemporaryDirectory() as directory:
            skill_path = Path(directory) / "SKILL.md"
            skill_path.write_text("vm skill", encoding="utf-8")

            with mock.patch.dict(
                "os.environ",
                {"GLUON_CLI_SKILL_PATH": str(skill_path)},
            ):
                skill = ClaudeAgentClient(config).gluon_cli_skill()

        self.assertEqual(skill, "vm skill")


if __name__ == "__main__":
    unittest.main()
