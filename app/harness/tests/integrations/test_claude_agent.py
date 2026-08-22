import json
import tempfile
import unittest
from pathlib import Path

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

            attempt = ClaudeAgentClient(config, log_path).repair_stage(
                "parse-build",
                1,
                Path("/repo"),
                failed,
            )

            self.assertEqual(attempt.stage_name, "parse-build")
            record = json.loads(log_path.read_text(encoding="utf-8"))
            self.assertEqual(record["attempt"], 1)


if __name__ == "__main__":
    unittest.main()

