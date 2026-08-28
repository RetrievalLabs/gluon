import json
import asyncio
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from integrations.claude_agent import (
    ClaudeAgentClient,
    invokes_gluon_cli,
    is_allowed_gluon_db_command,
)
from models.command import CommandResult
from models.config import HarnessConfig


class FakeResultMessage:
    def __init__(self, result: str) -> None:
        self.result = result
        self.is_error = False
        self.num_turns = 1


class FakeSdkClient:
    def __init__(self) -> None:
        self.prompts: list[str] = []
        self.disconnected = False

    async def query(self, prompt: str) -> None:
        self.prompts.append(prompt)

    async def receive_messages(self):
        yield FakeResultMessage("fixed")

    async def disconnect(self) -> None:
        self.disconnected = True


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
            self.assertIn("Stderr:", prompt)
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
        self.assertIn("Task", options["tools"])
        self.assertIn("Bash", options["tools"])
        self.assertIn("Task", options["allowed_tools"])
        self.assertIn("Edit", options["allowed_tools"])
        self.assertEqual(options["permission_mode"], "dontAsk")
        self.assertEqual(options["max_turns"], 80)
        self.assertEqual(options["skills"], ["gluon-cli"])
        self.assertEqual(options["hooks"]["PreToolUse"][0].matcher, "Bash")
        self.assertIn("system_prompt", options)
        self.assertTrue(options["system_prompt"]["exclude_dynamic_sections"])

    def test_reuses_connected_agent_client(self) -> None:
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
        client = ClaudeAgentClient(config)
        sdk_client = FakeSdkClient()
        client._client = sdk_client
        client._client_repo_path = Path("/repo")

        try:
            first = client.run_agent(Path("/repo"), "first")
            second = client.run_agent(Path("/repo"), "second")
        finally:
            client.close()

        self.assertEqual(first, "fixed")
        self.assertEqual(second, "fixed")
        self.assertEqual(sdk_client.prompts, ["first", "second"])
        self.assertTrue(sdk_client.disconnected)

    def test_blocks_gluon_cli_bash_commands(self) -> None:
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
        client = ClaudeAgentClient(config)

        denied = asyncio.run(
            client.block_gluon_cli_hook(
                {"tool_input": {"command": "gluon-cli code-parser parse-build"}},
                None,
                {},
            )
        )
        allowed = asyncio.run(
            client.block_gluon_cli_hook(
                {
                    "tool_input": {
                        "command": (
                            "gluon-cli code-parser db rows --database db --table t"
                        )
                    }
                },
                None,
                {},
            )
        )
        local = asyncio.run(
            client.block_gluon_cli_hook(
                {"tool_input": {"command": "mvn test"}},
                None,
                {},
            )
        )

        self.assertEqual(
            denied["hookSpecificOutput"]["permissionDecision"],
            "deny",
        )
        self.assertEqual(
            allowed["hookSpecificOutput"]["permissionDecision"],
            "allow",
        )
        self.assertEqual(
            local["hookSpecificOutput"]["permissionDecision"],
            "allow",
        )

    def test_detects_gluon_cli_invocations(self) -> None:
        self.assertTrue(invokes_gluon_cli("/usr/local/bin/gluon-cli --help"))
        self.assertTrue(invokes_gluon_cli("env FOO=bar gluon code-parser"))
        self.assertFalse(invokes_gluon_cli("ls /opt/gluon"))

    def test_allows_only_gluon_database_commands(self) -> None:
        self.assertTrue(
            is_allowed_gluon_db_command(
                "gluon-cli code-parser db rows --database db --table t"
            )
        )
        self.assertTrue(
            is_allowed_gluon_db_command(
                "env FOO=bar gluon code-parser db tables --database db"
            )
        )
        self.assertTrue(is_allowed_gluon_db_command("gluon-cli db tables --database db"))
        self.assertTrue(
            is_allowed_gluon_db_command(
                "gluon-cli db insert --database db --table t --set id=one"
            )
        )
        self.assertFalse(
            is_allowed_gluon_db_command("gluon-cli code-parser parse-build")
        )

    def test_characterization_prompt_orders_agent_handoff(self) -> None:
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
        prompt = ClaudeAgentClient(config).build_characterization_prompt(
            {"scenario_id": "scenario:one"}
        )

        self.assertIn("You are the main agent", prompt)
        self.assertIn("Use the Task tool to give the seed context", prompt)
        self.assertIn("Context Agent returns structured JSON", prompt)
        self.assertIn("Implementation Agent writes", prompt)
        self.assertIn("Input/Output Agent enumerates every generated `@Test` method", prompt)
        self.assertIn("one row per test method into `characterization_inputs`", prompt)
        self.assertIn("input_json.method", prompt)
        self.assertIn("db insert", prompt)
        self.assertIn("Return control to harness", prompt)
        self.assertIn("Do not select the next scenario", prompt)

    def test_dependency_selection_agent_options_allow_web_research(self) -> None:
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

        options = ClaudeAgentClient(config).dependency_selection_agent_options_kwargs(
            Path("/rewrite")
        )

        self.assertEqual(options["cwd"], Path("/rewrite"))
        self.assertIn("WebSearch", options["tools"])
        self.assertIn("WebFetch", options["allowed_tools"])
        self.assertIn("Write", options["allowed_tools"])
        self.assertEqual(options["skills"], ["java-dependency-selection-best-practices"])
        self.assertEqual(options["permission_mode"], "dontAsk")

    def test_dependency_selection_prompt_names_inputs_and_output(self) -> None:
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
        prompt = ClaudeAgentClient(config).build_dependency_selection_prompt(
            Path("/repo"),
            Path("/data/project/build-report.json"),
            Path("/data/project/compatibility-report.json"),
            "25",
            Path("/rewrite/docs/migration/dependency-selection.md"),
        )

        self.assertIn("Build report: /data/project/build-report.json", prompt)
        self.assertIn(
            "Compatibility report: /data/project/compatibility-report.json",
            prompt,
        )
        self.assertIn("Target Java version: 25", prompt)
        self.assertIn("/rewrite/docs/migration/dependency-selection.md", prompt)
        self.assertIn("Parent Dependencies", prompt)
        self.assertIn("Module Dependencies", prompt)
        self.assertIn("WebSearch and WebFetch", prompt)
        self.assertIn("java-dependency-selection-best-practices", prompt)

    def test_build_structure_agent_options_allow_build_writes_without_bash(self) -> None:
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

        options = ClaudeAgentClient(config).build_structure_agent_options_kwargs(
            Path("/rewrite")
        )

        self.assertEqual(options["cwd"], Path("/rewrite"))
        self.assertIn("Write", options["allowed_tools"])
        self.assertIn("MultiEdit", options["allowed_tools"])
        self.assertIn("WebSearch", options["tools"])
        self.assertIn("WebFetch", options["allowed_tools"])
        self.assertNotIn("Bash", options["tools"])
        self.assertEqual(options["skills"], ["java-build-tool-best-practices"])

    def test_build_structure_prompt_limits_legacy_reads(self) -> None:
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
        prompt = ClaudeAgentClient(config).build_build_structure_prompt(
            Path("/legacy"),
            Path("/data/project/build-report.json"),
            Path("/data/project/compatibility-report.json"),
            Path("/rewrite/docs/migration/legacy-tree"),
            Path("/rewrite/docs/migration/dependency-selection.md"),
            Path("/rewrite/docs/migration/build-structure.md"),
            "25",
            [
                Path("/legacy/pom.xml"),
                Path("/legacy/service/pom.xml"),
            ],
        )

        self.assertIn("Legacy repository reference: /legacy", prompt)
        self.assertIn("/data/project/build-report.json", prompt)
        self.assertIn("/rewrite/docs/migration/dependency-selection.md", prompt)
        self.assertIn("/rewrite/docs/migration/build-structure.md", prompt)
        self.assertIn("- /legacy/pom.xml", prompt)
        self.assertIn("- /legacy/service/pom.xml", prompt)
        self.assertIn("Do not read legacy source files", prompt)
        self.assertIn("java-build-tool-best-practices", prompt)
        self.assertIn("Create module directories", prompt)

    def test_source_migration_agent_options_use_task_and_java_skills(self) -> None:
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

        options = ClaudeAgentClient(config).source_migration_agent_options_kwargs(
            Path("/rewrite")
        )

        self.assertEqual(options["cwd"], Path("/rewrite"))
        self.assertIn("Task", options["tools"])
        self.assertIn("Bash", options["allowed_tools"])
        self.assertIn("WebSearch", options["tools"])
        self.assertIn("WebFetch", options["allowed_tools"])
        self.assertIn("version-rewrite-modernization", options["skills"])
        self.assertIn("java-best-practices", options["skills"])

    def test_source_migration_prompt_uses_databases_and_multi_agent_flow(self) -> None:
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
        prompt = ClaudeAgentClient(config).build_source_migration_prompt(
            Path("/legacy"),
            Path("/data/business-kg.db"),
            Path("/data/extraction.db"),
            Path("/legacy/gluon/tests/characterization-tests.db"),
            Path("/legacy/gluon/tests"),
            "25",
            Path("/rewrite/docs/migration/source-migration.md"),
        )

        self.assertIn("Legacy repository reference: /legacy", prompt)
        self.assertIn("Business KG database: /data/business-kg.db", prompt)
        self.assertIn("Extraction database: /data/extraction.db", prompt)
        self.assertIn(
            "Characterization database: /legacy/gluon/tests/characterization-tests.db",
            prompt,
        )
        self.assertIn("Context Agent", prompt)
        self.assertIn("Implementation Agent", prompt)
        self.assertIn("Verification Agent", prompt)
        self.assertIn("official web documentation", prompt)
        self.assertIn("integration tests", prompt)
        self.assertIn("characterization-tests.db", prompt)
        self.assertIn("Do not use build-report.json", prompt)
        self.assertIn("version-rewrite-modernization", prompt)
        self.assertIn("java-best-practices", prompt)


if __name__ == "__main__":
    unittest.main()
