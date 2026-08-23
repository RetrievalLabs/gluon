import os

from errors import StageFailedError
from execution.commands import CommandRunner
from execution.git_workspace import GitWorkspace
from execution.paths import HarnessPaths
from integrations.backend import BackendClient
from integrations.claude_agent import ClaudeAgentClient
from integrations.gluon_cli import GluonCli
from models.config import HarnessConfig
from models.summary import RunSummary
from pipeline.characterization import run_characterization_agent_loop
from pipeline.retry import run_stage_with_repair
from pipeline.stages import build_stages
from pipeline.summary import write_summary


class PipelineCoordinator:
    def __init__(self, config: HarnessConfig, env: dict[str, str]) -> None:
        self.config = config
        self.env = env
        self.paths = HarnessPaths.from_org_project(config.org_project_name)

    def run(self) -> RunSummary:
        self.paths.root.mkdir(parents=True, exist_ok=True)
        runner = CommandRunner(self.paths.command_log)
        agent = ClaudeAgentClient(self.config, self.paths.agent_log)
        backend = BackendClient(self.config, self.env)
        repo = backend.fetch_repo()
        agent.validate()
        GitWorkspace(runner).prepare(repo, self.paths.repo, self.config.target_version)

        stage_env = dict(os.environ)
        stage_env.update(self.env)
        stage_env["JAVA_HOME"] = f"/opt/jdks/jdk{self.config.current_version}"

        gluon = GluonCli(self.config.gluon_cli, self.paths)
        completed: list[str] = []
        try:
            try:
                for stage in build_stages(self.config, gluon):
                    result = run_stage_with_repair(
                        stage,
                        runner,
                        agent,
                        self.paths.repo,
                        self.config.max_agent_attempts,
                        stage_env,
                    )
                    target = gluon.write_stdout_target(stage.command)
                    if target is not None:
                        target.write_text(result.command_result.stdout, encoding="utf-8")
                    completed.append(stage.name)
                    if stage.name == "generate-characterization-tests":
                        scenarios = run_characterization_agent_loop(
                            self.paths,
                            agent,
                            runner,
                        )
                        if scenarios:
                            completed.append("characterization-full-tests")
            except StageFailedError as error:
                summary = RunSummary(
                    status="failed",
                    completed_stages=completed,
                    failed_stage=stage.name,
                    attempts_used=self.config.max_agent_attempts,
                    message=str(error),
                )
                write_summary(self.paths.summary, summary)
                raise
        finally:
            agent.close()

        summary = RunSummary(status="ok", completed_stages=completed)
        write_summary(self.paths.summary, summary)
        return summary
