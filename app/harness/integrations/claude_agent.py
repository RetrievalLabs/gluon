import asyncio
import json
import os
import shlex
from pathlib import Path
from types import SimpleNamespace
from typing import Any

from models.agent import AgentAttempt
from models.command import CommandResult
from models.config import HarnessConfig

OUTPUT_EXCERPT_LIMIT = 12000


class ClaudeAgentClient:
    def __init__(self, config: HarnessConfig, log_path: Path | None = None) -> None:
        self.config = config
        self.log_path = log_path
        self._loop: asyncio.AbstractEventLoop | None = None
        self._client: Any | None = None
        self._client_repo_path: Path | None = None

    def validate(self) -> None:
        # Import lazily so offline tests can run without the SDK installed.
        __import__("claude_agent_sdk")

    def repair_stage(
        self,
        stage_name: str,
        attempt: int,
        repo_path: Path,
        failed: CommandResult,
    ) -> AgentAttempt:
        command = shlex.join(failed.command)
        prompt = self.build_repair_prompt(stage_name, attempt, repo_path, failed)
        agent_result = self.run_agent(repo_path, prompt)
        message = (
            f"repair completed for {stage_name} attempt {attempt} "
            f"in {repo_path}: command `{command}` cwd `{failed.cwd}` "
            f"exit {failed.exit_code}; {agent_result}"
        )
        result = AgentAttempt(
            stage_name=stage_name,
            attempt=attempt,
            status="completed",
            message=message,
        )
        self.record(result)
        return result

    def run_agent(self, repo_path: Path, prompt: str) -> str:
        return self.event_loop().run_until_complete(
            self.run_agent_async(repo_path, prompt)
        )

    async def run_agent_async(self, repo_path: Path, prompt: str) -> str:
        client = await self.agent_client(repo_path)
        await client.query(prompt)
        result_text = "no result message returned"
        async for message in client.receive_messages():
            if hasattr(message, "result") and getattr(message, "result"):
                result_text = str(getattr(message, "result"))
            if getattr(message, "is_error", False):
                errors = getattr(message, "errors", None)
                detail = ", ".join(errors) if errors else result_text
                raise RuntimeError(f"Claude agent repair failed: {detail}")
            if hasattr(message, "num_turns"):
                break
        return excerpt(result_text)

    async def agent_client(self, repo_path: Path) -> Any:
        if self._client is not None:
            if self._client_repo_path != repo_path:
                raise RuntimeError("Claude agent session cannot switch repositories")
            return self._client

        from claude_agent_sdk import ClaudeAgentOptions, ClaudeSDKClient

        options = ClaudeAgentOptions(**self.agent_options_kwargs(repo_path))
        self._client = ClaudeSDKClient(options=options)
        self._client_repo_path = repo_path
        await self._client.connect()
        return self._client

    def event_loop(self) -> asyncio.AbstractEventLoop:
        if self._loop is None:
            self._loop = asyncio.new_event_loop()
        return self._loop

    def close(self) -> None:
        if self._client is None:
            if self._loop is not None:
                self._loop.close()
                self._loop = None
            return
        loop = self.event_loop()
        try:
            loop.run_until_complete(self._client.disconnect())
        finally:
            self._client = None
            self._client_repo_path = None
            loop.close()
            self._loop = None

    def agent_options_kwargs(self, repo_path: Path) -> dict[str, Any]:
        return {
            "model": self.config.anthropic_model,
            "cwd": repo_path,
            "tools": [
                "Task",
                "Bash",
                "Read",
                "Edit",
                "MultiEdit",
                "Glob",
                "Grep",
                "LS",
            ],
            "allowed_tools": [
                "Task",
                "Bash",
                "Read",
                "Edit",
                "MultiEdit",
                "Glob",
                "Grep",
                "LS",
            ],
            "permission_mode": "dontAsk",
            "hooks": {
                "PreToolUse": [
                    SimpleNamespace(
                        matcher="Bash",
                        hooks=[self.block_gluon_cli_hook],
                    )
                ]
            },
            "max_turns": 80,
            "skills": ["gluon-cli"],
            "system_prompt": {
                "type": "preset",
                "preset": "claude_code",
                "append": self.system_prompt(),
                "exclude_dynamic_sections": True,
            },
            "env": self.agent_env(),
        }

    def agent_env(self) -> dict[str, str]:
        env = dict(os.environ)
        env["ANTHROPIC_API_KEY"] = self.config.anthropic_api_key
        env["ANTHROPIC_MODEL"] = self.config.anthropic_model
        env["ANTHROPIC_BASE_URL"] = self.config.anthropic_base_url
        env["ANTHROPIC_API_BASE"] = self.config.anthropic_base_url
        env["GLUON_CLI"] = self.config.gluon_cli
        return env

    def system_prompt(self) -> str:
        return """You are a repair agent for the Gluon Java modernization harness.

Goal: make the failed Gluon CLI stage pass, then stop.

Rules:
- Work only inside the provided repository checkout unless reading Gluon CLI context supplied in the prompt.
- Diagnose from the failed command, cwd, exit code, stdout, stderr, and generated artifact paths.
- Make the smallest code or build-file change that fixes the failed stage.
- Preserve existing Java behavior. Do not modernize unrelated code.
- Do not skip harness stages, disable checks, delete source, rewrite history, or run destructive git commands.
- Do not run Gluon pipeline commands. Harness reruns failed stages after repair.
- Only `gluon[-cli] code-parser db ...` or `gluon[-cli] db ...` commands are allowed for characterization database work when the prompt asks for it.
- Prefer local project conventions and existing tests.
- Verify with local build or tests when useful, but leave Gluon CLI stage reruns to harness.
- Report changed files, verification command, and remaining blocker if any.
"""

    async def block_gluon_cli_hook(
        self,
        hook_input: dict[str, Any],
        _tool_use_id: str | None,
        _context: Any,
    ) -> dict[str, Any]:
        command = str(hook_input.get("tool_input", {}).get("command", ""))
        if invokes_gluon_cli(command) and not is_allowed_gluon_db_command(command):
            return {
                "hookSpecificOutput": {
                    "hookEventName": "PreToolUse",
                    "permissionDecision": "deny",
                    "permissionDecisionReason": (
                        "Harness reruns Gluon pipeline stages. Agents may only "
                        "run `gluon[-cli] code-parser db ...` or "
                        "`gluon[-cli] db ...` commands."
                    ),
                }
            }
        return {
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "permissionDecision": "allow",
            }
        }

    def build_repair_prompt(
        self,
        stage_name: str,
        attempt: int,
        repo_path: Path,
        failed: CommandResult,
    ) -> str:
        command = shlex.join(failed.command)
        return f"""Repair failed Gluon harness stage.

Stage: {stage_name}
Attempt: {attempt}
Repository: {repo_path}
Command: {command}
Command cwd: {failed.cwd}
Exit code: {failed.exit_code}
Elapsed ms: {failed.elapsed_ms}

Stdout:
```text
{excerpt(failed.stdout)}
```

Stderr:
```text
{excerpt(failed.stderr)}
```
"""

    def run_characterization_scenario(
        self,
        scenario_id: str,
        repo_path: Path,
        seed_context: dict[str, Any],
    ) -> AgentAttempt:
        prompt = self.build_characterization_prompt(seed_context)
        agent_result = self.run_agent(repo_path, prompt)
        result = AgentAttempt(
            stage_name="characterization-full-test",
            attempt=1,
            status="completed",
            message=(
                f"characterization scenario {scenario_id} completed in "
                f"{repo_path}: {agent_result}"
            ),
        )
        self.record(result)
        return result

    def build_characterization_prompt(self, seed_context: dict[str, Any]) -> str:
        return f"""Generate one full characterization test from harness seed context.

You are the main agent. Follow this order exactly:

1. Read the seed context below.
2. Use the Task tool to give the seed context to the Context Agent.
3. Context Agent returns structured JSON context packet only.
4. As main agent, use the Task tool to give the context packet and implementation responsibility to the Implementation Agent.
5. Implementation Agent writes the executable project-native characterization test using mocks or fakes for external dependencies, then runs the project-local test command and repairs compile/test failures.
6. As main agent, use the Task tool to give the written test and context packet to the Input/Output Agent.
7. Input/Output Agent enumerates every generated `@Test` method, generates deterministic inputs for each method, reruns the written test with those inputs, captures observed outputs, inserts one row per test method into `characterization_inputs`, inserts one linked row per test method into `characterization_observations`, and updates the scenario status to `accepted` in `characterization-tests.db` using only Gluon CLI database commands.
8. Verify with the project-local build/test command after database writes.
9. Return control to harness only after the test passes, inputs are stored, observations are stored, and scenario status is `accepted`. Do not select the next scenario.

Rules:
- Work only on the selected scenario.
- Do not modify production source or user-authored tests.
- Do not invent expected outputs. Outputs must come from running the written test with generated inputs.
- Store each test method name in `characterization_inputs.input_json.method`; harness uses that to verify coverage.
- Do not run Gluon pipeline commands. You may run only `gluon[-cli] code-parser db ...` or `gluon[-cli] db ...` database commands.
- Use `gluon[-cli] ... db insert` for new input and observation rows. Use `gluon[-cli] ... db update` for scenario status.
- Use git status/diff for review, but do not commit. Harness commits after control returns.
- Keep generated tests deterministic.
- If any required database write fails, fix it before returning control.

Seed context:
```json
{json.dumps(seed_context, indent=2, sort_keys=True)}
```
"""

    def record(self, attempt: AgentAttempt) -> None:
        if self.log_path is None:
            return
        self.log_path.parent.mkdir(parents=True, exist_ok=True)
        with self.log_path.open("a", encoding="utf-8") as handle:
            handle.write(json.dumps(attempt.to_dict(), sort_keys=True))
            handle.write("\n")


def excerpt(value: str, limit: int = OUTPUT_EXCERPT_LIMIT) -> str:
    if len(value) <= limit:
        return value
    half = limit // 2
    return (
        value[:half]
        + f"\n... truncated {len(value) - limit} characters ...\n"
        + value[-half:]
    )


def invokes_gluon_cli(command: str) -> bool:
    try:
        tokens = shlex.split(command)
    except ValueError:
        tokens = command.split()
    expect_command = True
    for token in tokens:
        if token in {";", "&&", "||", "|"}:
            expect_command = True
            continue
        if not expect_command:
            continue
        if "=" in token and token.split("=", 1)[0].isidentifier():
            continue
        if token in {"env", "sudo", "command"}:
            continue
        if Path(token).name in {"gluon", "gluon-cli"}:
            return True
        expect_command = False
    return False


def is_allowed_gluon_db_command(command: str) -> bool:
    try:
        tokens = shlex.split(command)
    except ValueError:
        return False
    expect_command = True
    for index, token in enumerate(tokens):
        if token in {";", "&&", "||", "|"}:
            expect_command = True
            continue
        if not expect_command:
            continue
        if "=" in token and token.split("=", 1)[0].isidentifier():
            continue
        if token in {"env", "sudo", "command"}:
            continue
        if Path(token).name in {"gluon", "gluon-cli"}:
            return tokens[index + 1 : index + 3] == [
                "code-parser",
                "db",
            ] or tokens[index + 1 : index + 2] == ["db"]
        expect_command = False
    return False
