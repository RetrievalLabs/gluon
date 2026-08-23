from contextlib import closing
import sqlite3
import tempfile
import unittest
from pathlib import Path

from db_contracts import (
    characterization_field,
    characterization_scenario_status,
    characterization_table,
)
from errors import StageFailedError
from execution.paths import HarnessPaths
from generated.gluon.db.v1 import characterization_tests_pb2
from pipeline.characterization import run_characterization_agent_loop

BEHAVIORS_TABLE = characterization_table(
    characterization_tests_pb2.CHARACTERIZATION_TABLE_BEHAVIORS
)
SCENARIOS_TABLE = characterization_table(
    characterization_tests_pb2.CHARACTERIZATION_TABLE_SCENARIOS
)
FILES_TABLE = characterization_table(
    characterization_tests_pb2.CHARACTERIZATION_TABLE_FILES
)
INPUTS_TABLE = characterization_table(
    characterization_tests_pb2.CHARACTERIZATION_TABLE_INPUTS
)
OBSERVATIONS_TABLE = characterization_table(
    characterization_tests_pb2.CHARACTERIZATION_TABLE_OBSERVATIONS
)
ACCEPTED = characterization_scenario_status(
    characterization_tests_pb2.CHARACTERIZATION_SCENARIO_STATUS_ACCEPTED
)
GENERATED_SCAFFOLD = characterization_scenario_status(
    characterization_tests_pb2.CHARACTERIZATION_SCENARIO_STATUS_GENERATED_SCAFFOLD
)
SCENARIO_ID = characterization_field(
    characterization_tests_pb2.CharacterizationScenarioRow,
    "id",
)
SCENARIO_BEHAVIOR_ID = characterization_field(
    characterization_tests_pb2.CharacterizationScenarioRow,
    "behavior_id",
)
SCENARIO_NAME = characterization_field(
    characterization_tests_pb2.CharacterizationScenarioRow,
    "name",
)
SCENARIO_KIND = characterization_field(
    characterization_tests_pb2.CharacterizationScenarioRow,
    "scenario_kind",
)
SCENARIO_INVOCATION_KIND = characterization_field(
    characterization_tests_pb2.CharacterizationScenarioRow,
    "invocation_kind",
)
SCENARIO_STATUS = characterization_field(
    characterization_tests_pb2.CharacterizationScenarioRow,
    "status",
)
SCENARIO_DIAGNOSTIC_REASON = characterization_field(
    characterization_tests_pb2.CharacterizationScenarioRow,
    "diagnostic_reason",
)
BEHAVIOR_ID = characterization_field(
    characterization_tests_pb2.CharacterizationBehaviorRow,
    "id",
)
BEHAVIOR_KG_NODE_ID = characterization_field(
    characterization_tests_pb2.CharacterizationBehaviorRow,
    "kg_node_id",
)
FILE_SCENARIO_ID = characterization_field(
    characterization_tests_pb2.CharacterizationFileRow,
    "scenario_id",
)
FILE_PATH = characterization_field(
    characterization_tests_pb2.CharacterizationFileRow,
    "path",
)
INPUT_ID = characterization_field(
    characterization_tests_pb2.CharacterizationInputRow,
    "id",
)
INPUT_SCENARIO_ID = characterization_field(
    characterization_tests_pb2.CharacterizationInputRow,
    "scenario_id",
)
INPUT_JSON = characterization_field(
    characterization_tests_pb2.CharacterizationInputRow,
    "input_json",
)
FIXTURE_JSON = characterization_field(
    characterization_tests_pb2.CharacterizationInputRow,
    "fixture_json",
)
DETERMINISTIC_SEED_JSON = characterization_field(
    characterization_tests_pb2.CharacterizationInputRow,
    "deterministic_seed_json",
)
OBSERVATION_ID = characterization_field(
    characterization_tests_pb2.CharacterizationObservationRow,
    "id",
)
OBSERVATION_SCENARIO_ID = characterization_field(
    characterization_tests_pb2.CharacterizationObservationRow,
    "scenario_id",
)
OBSERVATION_INPUT_ID = characterization_field(
    characterization_tests_pb2.CharacterizationObservationRow,
    "input_id",
)
OBSERVATION_STATUS = characterization_field(
    characterization_tests_pb2.CharacterizationObservationRow,
    "status",
)
OBSERVATION_RETURN_VALUE_JSON = characterization_field(
    characterization_tests_pb2.CharacterizationObservationRow,
    "return_value_json",
)
OBSERVATION_RESPONSE_BODY = characterization_field(
    characterization_tests_pb2.CharacterizationObservationRow,
    "response_body",
)
OBSERVATION_EXCEPTION_TYPE = characterization_field(
    characterization_tests_pb2.CharacterizationObservationRow,
    "exception_type",
)
OBSERVATION_EXCEPTION_MESSAGE = characterization_field(
    characterization_tests_pb2.CharacterizationObservationRow,
    "exception_message",
)
OBSERVATION_EMITTED_EVENTS_JSON = characterization_field(
    characterization_tests_pb2.CharacterizationObservationRow,
    "emitted_events_json",
)
OBSERVATION_DATABASE_SIDE_EFFECTS_JSON = characterization_field(
    characterization_tests_pb2.CharacterizationObservationRow,
    "database_side_effects_json",
)
OBSERVATION_FAKE_BOUNDARY_CALLS_JSON = characterization_field(
    characterization_tests_pb2.CharacterizationObservationRow,
    "fake_boundary_calls_json",
)
OBSERVATION_NORMALIZED_OUTPUT_JSON = characterization_field(
    characterization_tests_pb2.CharacterizationObservationRow,
    "normalized_output_json",
)


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
        database = Path(seed_context["database_paths"]["characterization_tests"])
        with closing(sqlite3.connect(database)) as connection:
            with connection:
                connection.execute(
                    f"""
                    UPDATE {SCENARIOS_TABLE}
                    SET {SCENARIO_STATUS} = ?
                    WHERE {SCENARIO_ID} = ?
                    """,
                    [ACCEPTED, scenario_id],
                )
                connection.execute(
                    f"""
                    INSERT INTO {INPUTS_TABLE} (
                        {INPUT_ID}, {INPUT_SCENARIO_ID}, {INPUT_JSON},
                        {FIXTURE_JSON}, {DETERMINISTIC_SEED_JSON}
                    ) VALUES (?, ?, '{{}}', '{{}}', '{{}}')
                    """,
                    [f"input:{scenario_id}", scenario_id],
                )
                connection.execute(
                    f"""
                    INSERT INTO {OBSERVATIONS_TABLE} (
                        {OBSERVATION_ID}, {OBSERVATION_SCENARIO_ID},
                        {OBSERVATION_INPUT_ID}, {OBSERVATION_STATUS},
                        {OBSERVATION_EMITTED_EVENTS_JSON},
                        {OBSERVATION_DATABASE_SIDE_EFFECTS_JSON},
                        {OBSERVATION_FAKE_BOUNDARY_CALLS_JSON},
                        {OBSERVATION_NORMALIZED_OUTPUT_JSON}
                    ) VALUES (?, ?, ?, 'observed', '[]', '[]', '[]', '{{}}')
                    """,
                    [f"observation:{scenario_id}", scenario_id, f"input:{scenario_id}"],
                )


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
        self.assertIn(
            ["git", "add", "gluon/tests/Test.java", "gluon/tests/characterization-tests.db"],
            runner.commands,
        )
        self.assertIn(
            ["git", "commit", "-m", "Add characterization test for scenario:one"],
            runner.commands,
        )

    def test_fails_when_agent_does_not_store_inputs_and_outputs(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            paths = HarnessPaths.from_org_project("org/project", Path(directory))
            paths.characterization_db.parent.mkdir(parents=True)
            write_characterization_db(paths.characterization_db)
            agent = FakeAgentWithoutDatabaseWrites()

            with self.assertRaisesRegex(
                StageFailedError,
                "missing stored inputs or observations",
            ):
                run_characterization_agent_loop(paths, agent, FakeRunner())

    def test_retries_accepted_scenario_missing_inputs_and_outputs(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            paths = HarnessPaths.from_org_project("org/project", Path(directory))
            paths.characterization_db.parent.mkdir(parents=True)
            write_characterization_db(paths.characterization_db, status=ACCEPTED)
            agent = FakeAgent()

            completed = run_characterization_agent_loop(paths, agent, FakeRunner())

        self.assertEqual(completed, ["scenario:one"])
        self.assertEqual(len(agent.calls), 1)


def write_characterization_db(path: Path, status: str = GENERATED_SCAFFOLD) -> None:
    with closing(sqlite3.connect(path)) as connection:
        with connection:
            connection.executescript(
                f"""
                CREATE TABLE {BEHAVIORS_TABLE} (
                    {BEHAVIOR_ID} TEXT PRIMARY KEY,
                    {BEHAVIOR_KG_NODE_ID} TEXT NOT NULL
                );
                CREATE TABLE {SCENARIOS_TABLE} (
                    {SCENARIO_ID} TEXT PRIMARY KEY,
                    {SCENARIO_BEHAVIOR_ID} TEXT NOT NULL,
                    {SCENARIO_NAME} TEXT NOT NULL,
                    {SCENARIO_KIND} TEXT NOT NULL,
                    {SCENARIO_INVOCATION_KIND} TEXT,
                    {SCENARIO_STATUS} TEXT NOT NULL,
                    {SCENARIO_DIAGNOSTIC_REASON} TEXT
                );
                CREATE TABLE {FILES_TABLE} (
                    {FILE_SCENARIO_ID} TEXT NOT NULL,
                    {FILE_PATH} TEXT NOT NULL
                );
                CREATE TABLE {INPUTS_TABLE} (
                    {INPUT_ID} TEXT PRIMARY KEY,
                    {INPUT_SCENARIO_ID} TEXT NOT NULL,
                    {INPUT_JSON} TEXT NOT NULL,
                    {FIXTURE_JSON} TEXT NOT NULL,
                    {DETERMINISTIC_SEED_JSON} TEXT NOT NULL
                );
                CREATE TABLE {OBSERVATIONS_TABLE} (
                    {OBSERVATION_ID} TEXT PRIMARY KEY,
                    {OBSERVATION_SCENARIO_ID} TEXT NOT NULL,
                    {OBSERVATION_INPUT_ID} TEXT NOT NULL,
                    {OBSERVATION_STATUS} TEXT NOT NULL,
                    {OBSERVATION_RETURN_VALUE_JSON} TEXT,
                    {OBSERVATION_RESPONSE_BODY} TEXT,
                    {OBSERVATION_EXCEPTION_TYPE} TEXT,
                    {OBSERVATION_EXCEPTION_MESSAGE} TEXT,
                    {OBSERVATION_EMITTED_EVENTS_JSON} TEXT NOT NULL,
                    {OBSERVATION_DATABASE_SIDE_EFFECTS_JSON} TEXT NOT NULL,
                    {OBSERVATION_FAKE_BOUNDARY_CALLS_JSON} TEXT NOT NULL,
                    {OBSERVATION_NORMALIZED_OUTPUT_JSON} TEXT NOT NULL
                );
                INSERT INTO {BEHAVIORS_TABLE} ({BEHAVIOR_ID}, {BEHAVIOR_KG_NODE_ID})
                VALUES ('behavior:one', 'node:one');
                INSERT INTO {SCENARIOS_TABLE} (
                    {SCENARIO_ID}, {SCENARIO_BEHAVIOR_ID}, {SCENARIO_NAME},
                    {SCENARIO_KIND}, {SCENARIO_INVOCATION_KIND}, {SCENARIO_STATUS},
                    {SCENARIO_DIAGNOSTIC_REASON}
                ) VALUES (
                    'scenario:one', 'behavior:one', 'Approve', 'happy_path',
                    'method', '{status}', NULL
                );
                INSERT INTO {FILES_TABLE} ({FILE_SCENARIO_ID}, {FILE_PATH})
                VALUES ('scenario:one', 'gluon/tests/Test.java');
                """
            )


class FakeAgentWithoutDatabaseWrites:
    def run_characterization_scenario(
        self,
        scenario_id: str,
        repo_path: Path,
        seed_context: dict[str, object],
    ) -> None:
        database = Path(seed_context["database_paths"]["characterization_tests"])
        with closing(sqlite3.connect(database)) as connection:
            with connection:
                connection.execute(
                    f"""
                    UPDATE {SCENARIOS_TABLE}
                    SET {SCENARIO_STATUS} = ?
                    WHERE {SCENARIO_ID} = ?
                    """,
                    [ACCEPTED, scenario_id],
                )


if __name__ == "__main__":
    unittest.main()
