import unittest
from pathlib import Path

from errors import StageFailedError
from models.command import CommandResult
from models.stage import Stage
from pipeline.retry import run_stage_with_repair


class FakeRunner:
    def __init__(self) -> None:
        self.calls = 0

    def run(self, command, cwd=None, env=None):
        self.calls += 1
        return CommandResult(command, str(cwd), 1, "", "failed", 1)


class FakeAgent:
    def __init__(self) -> None:
        self.repairs = 0

    def repair_stage(self, stage_name, attempt, repo_path, failed):
        self.repairs += 1


class RetryTests(unittest.TestCase):
    def test_stops_after_max_attempts(self) -> None:
        runner = FakeRunner()
        agent = FakeAgent()
        stage = Stage("parse-build", ["false"], "/tmp/repo")

        with self.assertRaises(StageFailedError):
            run_stage_with_repair(stage, runner, agent, Path("/tmp/repo"), 2, {})

        self.assertEqual(runner.calls, 3)
        self.assertEqual(agent.repairs, 2)


if __name__ == "__main__":
    unittest.main()

