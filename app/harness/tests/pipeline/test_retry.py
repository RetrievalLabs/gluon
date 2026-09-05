import unittest
from pathlib import Path

from errors import StageFailedError
from models.command import CommandResult
from models.stage import Stage
from pipeline.retry import run_stage_with_repair


class FakeRunner:
    def __init__(self, events: list[str] | None = None) -> None:
        self.calls = 0
        self.events = events

    def run(self, command, cwd=None, env=None):
        self.calls += 1
        if self.events is not None:
            self.events.append(f"run:{self.calls}")
        return CommandResult(command, str(cwd), 1, "", "failed", 1)


class FakeAgent:
    def __init__(self, events: list[str] | None = None) -> None:
        self.repairs = 0
        self.events = events

    def repair_stage(self, stage_name, attempt, repo_path, failed):
        self.repairs += 1
        if self.events is not None:
            self.events.append(f"repair:{attempt}")
            self.events.append(f"compact:{attempt}")


class RetryTests(unittest.TestCase):
    def test_stops_after_max_attempts(self) -> None:
        runner = FakeRunner()
        agent = FakeAgent()
        stage = Stage("parse-build", ["false"], "/tmp/repo")

        with self.assertRaises(StageFailedError):
            run_stage_with_repair(stage, runner, agent, Path("/tmp/repo"), 2, {})

        self.assertEqual(runner.calls, 3)
        self.assertEqual(agent.repairs, 2)

    def test_repairs_and_compacts_before_next_stage_rerun(self) -> None:
        events: list[str] = []
        runner = FakeRunner(events)
        agent = FakeAgent(events)
        stage = Stage("parse-build", ["false"], "/tmp/repo")

        with self.assertRaises(StageFailedError):
            run_stage_with_repair(stage, runner, agent, Path("/tmp/repo"), 2, {})

        self.assertEqual(
            events,
            [
                "run:1",
                "repair:1",
                "compact:1",
                "run:2",
                "repair:2",
                "compact:2",
                "run:3",
            ],
        )


if __name__ == "__main__":
    unittest.main()
