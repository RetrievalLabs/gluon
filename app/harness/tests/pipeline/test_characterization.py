import sqlite3
import tempfile
import unittest
from pathlib import Path

from execution.paths import HarnessPaths
from pipeline.characterization import run_characterization_agent_loop


class FakeAgent:
    def __init__(self) -> None:
        self.calls: list[tuple[str, Path, dict[str, object]]] = []

    def run_characterization_scenario(
        self,
        scenario_id: str,
        repo_path: Path,
        seed_context: dict[str, object],
    ) -> None:
        self.calls.append((scenario_id, repo_path, seed_context))


class FakeRunner:
    def __init__(self) -> None:
        self.commands: list[list[str]] = []

    def run(self, command: list[str], cwd: Path | None = None, env=None):
        self.commands.append(command)
        stdout = (
            " M gluon/tests/GeneratedTest.java\n"
            if command[:2] == ["git", "status"]
            else ""
        )
        return type(
            "Result",
            (),
            {"stdout": stdout, "exit_code": 0, "stderr": "", "ok": True},
        )()


class CharacterizationLoopTests(unittest.TestCase):
    def test_skips_when_characterization_database_is_missing(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            paths = HarnessPaths.from_org_project("org/project", Path(directory))
            agent = FakeAgent()

            completed = run_characterization_agent_loop(paths, agent, FakeRunner())

        self.assertEqual(completed, [])
        self.assertEqual(agent.calls, [])

    def test_selects_scenario_collects_seed_and_returns_control_to_harness(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            paths = HarnessPaths.from_org_project("org/project", Path(directory))
            paths.characterization_db.parent.mkdir(parents=True)
            write_characterization_db(paths.characterization_db)
            agent = FakeAgent()
            runner = FakeRunner()

            completed = run_characterization_agent_loop(paths, agent, runner)

        self.assertEqual(completed, ["scenario:one"])
        self.assertEqual(len(agent.calls), 1)
        scenario_id, repo_path, seed_context = agent.calls[0]
        self.assertEqual(scenario_id, "scenario:one")
        self.assertEqual(repo_path, paths.repo)
        self.assertEqual(seed_context["scenario_id"], "scenario:one")
        self.assertEqual(seed_context["behavior_id"], "behavior:one")
        self.assertEqual(seed_context["kg_node_id"], "node:one")
        self.assertEqual(
            seed_context["abstract_scaffold_path"],
            "gluon/tests/Test.java",
        )
        self.assertEqual(
            seed_context["database_paths"]["characterization_tests"],
            str(paths.characterization_db),
        )
        self.assertIn(["git", "status", "--short"], runner.commands)
        self.assertIn(["git", "add", "gluon/tests"], runner.commands)
        self.assertIn(
            ["git", "commit", "-m", "Add characterization test for scenario:one"],
            runner.commands,
        )


def write_characterization_db(path: Path) -> None:
    with sqlite3.connect(path) as connection:
        connection.executescript(
            """
            CREATE TABLE characterization_behaviors (
                id TEXT PRIMARY KEY,
                kg_node_id TEXT NOT NULL
            );
            CREATE TABLE characterization_scenarios (
                id TEXT PRIMARY KEY,
                behavior_id TEXT NOT NULL,
                name TEXT NOT NULL,
                scenario_kind TEXT NOT NULL,
                invocation_kind TEXT,
                status TEXT NOT NULL,
                diagnostic_reason TEXT
            );
            CREATE TABLE characterization_files (
                scenario_id TEXT NOT NULL,
                path TEXT NOT NULL
            );
            INSERT INTO characterization_behaviors (id, kg_node_id)
            VALUES ('behavior:one', 'node:one');
            INSERT INTO characterization_scenarios (
                id, behavior_id, name, scenario_kind, invocation_kind, status,
                diagnostic_reason
            ) VALUES (
                'scenario:one', 'behavior:one', 'Approve', 'happy_path',
                'method', 'generated_scaffold', NULL
            );
            INSERT INTO characterization_files (scenario_id, path)
            VALUES ('scenario:one', 'gluon/tests/Test.java');
            """
        )


if __name__ == "__main__":
    unittest.main()
