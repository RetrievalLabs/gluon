import asyncio
import json
import os
import shlex
from pathlib import Path
from types import SimpleNamespace
from typing import Any

from models.agent import AgentAttempt, parse_agent_json_response
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
        prompt = self.build_repair_prompt(stage_name, attempt, repo_path, failed)
        agent_result = self.run_agent(repo_path, prompt)
        result = AgentAttempt(
            stage_name=stage_name,
            attempt=attempt,
            status="completed",
            message=agent_result,
        )
        self.record(result)
        self.compact_agent_context(repo_path, stage_name, attempt)
        return result

    def run_agent(self, repo_path: Path, prompt: str) -> str:
        return self.event_loop().run_until_complete(
            self.run_agent_async(repo_path, prompt)
        )

    def run_agent_with_options(self, options_kwargs: dict[str, Any], prompt: str) -> str:
        return self.event_loop().run_until_complete(
            self.run_agent_with_options_async(options_kwargs, prompt)
        )

    async def run_agent_async(self, repo_path: Path, prompt: str) -> str:
        client = await self.agent_client(repo_path)
        await client.query(prompt)
        return await self.receive_agent_result(client)

    def compact_agent_context(
        self,
        repo_path: Path,
        stage_name: str,
        attempt: int,
    ) -> None:
        prompt = self.build_compaction_prompt(repo_path, stage_name, attempt)
        self.event_loop().run_until_complete(
            self.compact_agent_context_async(repo_path, prompt)
        )

    def compact_characterization_context(
        self,
        repo_path: Path,
        scenario_id: str,
    ) -> None:
        prompt = self.build_characterization_compaction_prompt(repo_path, scenario_id)
        self.event_loop().run_until_complete(
            self.compact_agent_context_async(repo_path, prompt)
        )

    async def compact_agent_context_async(self, repo_path: Path, prompt: str) -> None:
        client = await self.agent_client(repo_path)
        await client.query(prompt)
        await self.receive_compaction_result(client)

    async def receive_compaction_result(self, client: Any) -> None:
        async for message in client.receive_messages():
            if getattr(message, "is_error", False):
                errors = getattr(message, "errors", None)
                detail = ", ".join(errors) if errors else "unknown error"
                raise RuntimeError(
                    f"Claude agent context compression failed: {detail}"
                )
            if hasattr(message, "num_turns"):
                break

    async def run_agent_with_options_async(
        self,
        options_kwargs: dict[str, Any],
        prompt: str,
    ) -> str:
        from claude_agent_sdk import ClaudeAgentOptions, ClaudeSDKClient

        client = ClaudeSDKClient(options=ClaudeAgentOptions(**options_kwargs))
        await client.connect()
        try:
            await client.query(prompt)
            return await self.receive_agent_result(client)
        finally:
            await client.disconnect()

    async def receive_agent_result(self, client: Any) -> str:
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
        return parse_agent_json_response(result_text).to_json()

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
                "WebSearch",
                "WebFetch",
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
                "WebSearch",
                "WebFetch",
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

    def dependency_selection_agent_options_kwargs(self, rewrite_workspace: Path) -> dict[str, Any]:
        return {
            "model": self.config.anthropic_model,
            "cwd": rewrite_workspace,
            "tools": [
                "Read",
                "Write",
                "Edit",
                "Glob",
                "Grep",
                "LS",
                "WebSearch",
                "WebFetch",
            ],
            "allowed_tools": [
                "Read",
                "Write",
                "Edit",
                "Glob",
                "Grep",
                "LS",
                "WebSearch",
                "WebFetch",
            ],
            "permission_mode": "dontAsk",
            "max_turns": 40,
            "skills": ["java-dependency-selection-best-practices"],
            "system_prompt": {
                "type": "preset",
                "preset": "claude_code",
                "append": self.dependency_selection_system_prompt(),
                "exclude_dynamic_sections": True,
            },
            "env": self.agent_env(),
        }

    def build_structure_agent_options_kwargs(self, rewrite_workspace: Path) -> dict[str, Any]:
        return {
            "model": self.config.anthropic_model,
            "cwd": rewrite_workspace,
            "tools": [
                "Read",
                "Write",
                "Edit",
                "MultiEdit",
                "Glob",
                "Grep",
                "LS",
                "WebSearch",
                "WebFetch",
            ],
            "allowed_tools": [
                "Read",
                "Write",
                "Edit",
                "MultiEdit",
                "Glob",
                "Grep",
                "LS",
                "WebSearch",
                "WebFetch",
            ],
            "permission_mode": "dontAsk",
            "max_turns": 60,
            "skills": ["java-build-tool-best-practices"],
            "system_prompt": {
                "type": "preset",
                "preset": "claude_code",
                "append": self.build_structure_system_prompt(),
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
""" + self.top_level_agent_json_contract()

    def dependency_selection_system_prompt(self) -> str:
        return """You are a dependency-selection agent for the Gluon Java modernization harness.

Goal: produce one Markdown dependency selection report, then stop.

Rules:
- Read the supplied build report and compatibility report before writing.
- Use the java-dependency-selection-best-practices skill.
- Use web search/fetch only to verify exact current stable versions from official project sources.
- Write only the supplied dependency-selection Markdown path.
- Do not edit source code, build files, generated reports, or other migration docs.
- Prefer platform-managed versions over manual dependency pins.
- Preserve existing dependency roles and avoid optional modernization unless required for target Java compatibility.
- Do not choose milestone, RC, snapshot, or development releases.
""" + self.top_level_agent_json_contract()

    def build_structure_system_prompt(self) -> str:
        return """You are a build-structure agent for the Gluon Java modernization harness.

Goal: create initial Maven or Gradle build structure in the rewrite workspace, then stop.

Rules:
- Use the java-build-tool-best-practices skill.
- Treat the legacy repository as read-only reference.
- Read only the explicitly allowed legacy build files supplied in the prompt.
- Do not read legacy source files, test files, resources, or arbitrary repository files.
- Write only inside the rewrite workspace.
- Preserve the legacy build system and module paths. Do not convert Maven to Gradle, Gradle to Maven, or Gradle Groovy DSL to Kotlin DSL.
- Use docs/migration/dependency-selection.md for selected dependency and platform versions.
- Use web search/fetch only to verify exact stable Maven, Gradle, or plugin versions from official project sources.
- Do not choose milestone, RC, snapshot, or development releases.
- Do not run build or test commands.
""" + self.top_level_agent_json_contract()

    def top_level_agent_json_contract(self) -> str:
        return """

Completion output:
- Work silently. Do not emit progress narration or human-facing summaries.
- Final response must be one compact JSON object only, with no Markdown fences and no text before or after it.
- Final response must match this schema exactly:
{"status":"completed","changed_files":["path"],"verification":[{"command":"cmd","status":"passed"}],"blockers":[]}
- Use `"status":"blocked"` with one or more short actionable blockers when work cannot complete.
- Keep generated code, docs, comments, and Markdown reports in normal professional prose; JSON-only applies only to agent completion output.
"""

    def subagent_json_contract(self) -> str:
        return (
            "Each subagent must work silently and return one task-specific JSON "
            "object only to the main agent. No Markdown fences, prose, progress "
            "narration, or text outside JSON. Main agent must read that JSON and "
            "use it to decide the next step. Treat every subagent Task call as "
            "a self-contained request/response handoff; do not rely on live "
            "subagent memory after the JSON is returned. Main agent may call "
            "the same subagent role again when its JSON reports missing "
            "context, failed verification, failed database writes, or blockers "
            "that the main agent can resolve, but must include prior JSON and "
            "current scenario state in the new task request."
        )

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

    def build_compaction_prompt(
        self,
        repo_path: Path,
        stage_name: str,
        attempt: int,
    ) -> str:
        return (
            "/compact Keep only facts needed to continue the Gluon CLI repair loop: "
            f"repository path {repo_path}, repaired stage {stage_name}, "
            f"repair attempt {attempt}, changed files, verification results, "
            "active blockers, current failed command context, and constraints. "
            "Drop raw stdout/stderr, tool traces, and stale exploration details."
        )

    def build_characterization_compaction_prompt(
        self,
        repo_path: Path,
        scenario_id: str,
    ) -> str:
        return (
            "/compact Keep only facts needed to continue the characterization "
            f"test loop: repository path {repo_path}, completed scenario "
            f"{scenario_id}, generated or changed test files, characterization "
            "database input/observation/status results, commit result if known, "
            "reusable lessons or blockers for later scenarios, and constraints. "
            "Preserve JSON subagent handoff rules, deterministic test "
            "requirements, and the rule against editing production source or "
            "user-authored tests. Drop raw logs, tool traces, and stale "
            "scenario exploration details."
        )

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
            message=agent_result,
        )
        self.record(result)
        return result

    def build_characterization_prompt(self, seed_context: dict[str, Any]) -> str:
        return f"""Generate one full characterization test from harness seed context.

You are the main agent. Follow this order exactly:

Subagent output contract: {self.subagent_json_contract()}

1. Read the seed context below.
2. Use the Task tool to give the seed context to the Context Agent.
3. Context Agent returns one structured JSON context packet only.
4. As main agent, use the Task tool to give the context packet and implementation responsibility to the Implementation Agent.
5. Implementation Agent writes the executable project-native characterization test using mocks or fakes for external dependencies, then runs the project-local test command, repairs compile/test failures, and returns one JSON object only.
6. As main agent, use the Task tool to give the written test and context packet to the Input/Output Agent.
7. Input/Output Agent enumerates every generated `@Test` method, generates deterministic inputs for each method, reruns the written test with those inputs, captures observed outputs, inserts one row per test method into `characterization_inputs`, inserts one linked row per test method into `characterization_observations`, updates the scenario status to `accepted` in `characterization-tests.db` using only Gluon CLI database commands, and returns one JSON object only.
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

    def run_dependency_selection(
        self,
        rewrite_workspace: Path,
        legacy_repo_path: Path,
        build_report_path: Path,
        compatibility_report_path: Path,
        target_version: str,
        output_path: Path,
    ) -> AgentAttempt:
        prompt = self.build_dependency_selection_prompt(
            legacy_repo_path,
            build_report_path,
            compatibility_report_path,
            target_version,
            output_path,
        )
        agent_result = self.run_agent_with_options(
            self.dependency_selection_agent_options_kwargs(rewrite_workspace),
            prompt,
        )
        result = AgentAttempt(
            stage_name="dependency-selection",
            attempt=1,
            status="completed",
            message=agent_result,
        )
        self.record(result)
        return result

    def build_dependency_selection_prompt(
        self,
        legacy_repo_path: Path,
        build_report_path: Path,
        compatibility_report_path: Path,
        target_version: str,
        output_path: Path,
    ) -> str:
        return f"""Generate Java dependency selection report.

Inputs:
- Legacy repository: {legacy_repo_path}
- Build report: {build_report_path}
- Compatibility report: {compatibility_report_path}
- Target Java version: {target_version}
- Output Markdown: {output_path}

Instructions:
1. Read build-report.json for parent and module dependency/plugin inventory.
2. Read compatibility-report.json for required Java compatibility recommendations.
3. Use java-dependency-selection-best-practices to select dependency versions or platform-managed versions that support target Java {target_version}.
4. Use WebSearch and WebFetch for exact current stable versions only when local reports/skill guidance do not provide enough evidence. Prefer official project documentation, release pages, or Maven Central pages.
5. Write exactly one Markdown file at `{output_path}`.

Markdown requirements:
- Title with target Java version.
- Parent Dependencies section with coordinates, current version, selected version, selection type, reason, and source.
- Module Dependencies section grouped by module path with the same fields.
- Build Plugins section when plugin recommendations or build tools exist.
- Unknown Inventory section for dependencies or plugins with no compatibility KB rule.
- Research Sources section listing official URLs used for version verification.

Rules:
- Do not edit any file except `{output_path}`.
- Do not modify build files.
- Prefer BOM/platform-managed versions where applicable.
- Mark managed dependencies as managed by platform instead of inventing direct pins.
- Use only stable versions. No milestone, RC, snapshot, or development releases.
- Keep report concise and actionable.
"""

    def run_build_structure(
        self,
        rewrite_workspace: Path,
        legacy_repo_path: Path,
        build_report_path: Path,
        compatibility_report_path: Path,
        legacy_tree_path: Path,
        dependency_selection_report_path: Path,
        build_structure_report_path: Path,
        target_version: str,
        allowed_legacy_build_files: list[Path],
    ) -> AgentAttempt:
        prompt = self.build_build_structure_prompt(
            legacy_repo_path,
            build_report_path,
            compatibility_report_path,
            legacy_tree_path,
            dependency_selection_report_path,
            build_structure_report_path,
            target_version,
            allowed_legacy_build_files,
        )
        agent_result = self.run_agent_with_options(
            self.build_structure_agent_options_kwargs(rewrite_workspace),
            prompt,
        )
        result = AgentAttempt(
            stage_name="build-structure",
            attempt=1,
            status="completed",
            message=agent_result,
        )
        self.record(result)
        return result

    def build_build_structure_prompt(
        self,
        legacy_repo_path: Path,
        build_report_path: Path,
        compatibility_report_path: Path,
        legacy_tree_path: Path,
        dependency_selection_report_path: Path,
        build_structure_report_path: Path,
        target_version: str,
        allowed_legacy_build_files: list[Path],
    ) -> str:
        allowed_files = "\n".join(f"- {path}" for path in allowed_legacy_build_files)
        if not allowed_files:
            allowed_files = "- No legacy build files were found by harness discovery."
        return f"""Create Java build structure in the rewrite workspace.

Inputs:
- Legacy repository reference: {legacy_repo_path}
- Build report: {build_report_path}
- Compatibility report: {compatibility_report_path}
- Legacy tree: {legacy_tree_path}
- Dependency selection report: {dependency_selection_report_path}
- Build structure report: {build_structure_report_path}
- Target Java version: {target_version}

Allowed legacy build files:
{allowed_files}

Instructions:
1. Read the build report, compatibility report, legacy tree, and dependency selection report.
2. Read only the allowed legacy build files listed above. Do not read any other legacy repository files.
3. Use java-build-tool-best-practices to create root build files in the rewrite workspace matching the legacy build system.
4. Create module directories and module build files when build-report.json contains modules or the allowed build files show module builds.
5. Set Java release/source/target/toolchain settings for Java {target_version}.
6. Apply selected dependency and plugin versions from docs/migration/dependency-selection.md. Prefer BOM/platform-managed versions when that report says versions are managed.
7. Use WebSearch and WebFetch only when exact stable Maven, Gradle, wrapper, or plugin versions need official verification.
8. Write `{build_structure_report_path}` describing generated root structure, modules, allowed legacy build files read, build system choices, and blockers.

Rules:
- Do not read legacy source files, test files, resources, or arbitrary legacy files.
- Do not copy Java source code from the legacy repository.
- Do not write outside the rewrite workspace.
- Preserve Maven vs Gradle and Gradle DSL choice.
- Preserve module names and paths from the reports.
- Do not invent application dependencies absent from the reports or dependency selection doc.
- Do not run build or test commands.
- Use only stable versions. No milestone, RC, snapshot, or development releases.
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
