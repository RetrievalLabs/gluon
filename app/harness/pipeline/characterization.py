from contextlib import closing
import json
import re
import sqlite3
from pathlib import Path
from typing import Any

from db_contracts import (
    characterization_field,
    characterization_scenario_status,
    characterization_table,
)
from errors import StageFailedError
from execution.commands import CommandRunner
from execution.paths import HarnessPaths
from generated.gluon.db.v1 import characterization_tests_pb2
from integrations.claude_agent import ClaudeAgentClient

SCENARIOS_TABLE = characterization_table(
    characterization_tests_pb2.CHARACTERIZATION_TABLE_SCENARIOS
)
BEHAVIORS_TABLE = characterization_table(
    characterization_tests_pb2.CHARACTERIZATION_TABLE_BEHAVIORS
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
INPUT_SCENARIO_ID = characterization_field(
    characterization_tests_pb2.CharacterizationInputRow,
    "scenario_id",
)
INPUT_ID = characterization_field(
    characterization_tests_pb2.CharacterizationInputRow,
    "id",
)
INPUT_JSON = characterization_field(
    characterization_tests_pb2.CharacterizationInputRow,
    "input_json",
)
OBSERVATION_SCENARIO_ID = characterization_field(
    characterization_tests_pb2.CharacterizationObservationRow,
    "scenario_id",
)
OBSERVATION_INPUT_ID = characterization_field(
    characterization_tests_pb2.CharacterizationObservationRow,
    "input_id",
)
COMPLETED_SCENARIO_STATUSES = {
    characterization_scenario_status(
        characterization_tests_pb2.CHARACTERIZATION_SCENARIO_STATUS_ACCEPTED
    ),
    characterization_scenario_status(
        characterization_tests_pb2.CHARACTERIZATION_SCENARIO_STATUS_COMMITTED
    ),
    characterization_scenario_status(
        characterization_tests_pb2.CHARACTERIZATION_SCENARIO_STATUS_SKIPPED
    ),
}
ACCEPTED_SCENARIO_STATUS = characterization_scenario_status(
    characterization_tests_pb2.CHARACTERIZATION_SCENARIO_STATUS_ACCEPTED
)
COMMITTED_SCENARIO_STATUS = characterization_scenario_status(
    characterization_tests_pb2.CHARACTERIZATION_SCENARIO_STATUS_COMMITTED
)
SKIPPED_SCENARIO_STATUS = characterization_scenario_status(
    characterization_tests_pb2.CHARACTERIZATION_SCENARIO_STATUS_SKIPPED
)


def run_characterization_agent_loop(
    paths: HarnessPaths,
    agent: ClaudeAgentClient,
    runner: CommandRunner,
) -> list[str]:
    if not paths.characterization_db.exists():
        return []

    completed: list[str] = []
    processed: set[str] = set()
    for _ in range(count_incomplete_scenarios(paths.characterization_db, paths.repo)):
        scenario = select_next_scenario(paths, processed)
        if scenario is None:
            break

        scenario_id = str(scenario["scenario_id"])
        processed.add(scenario_id)
        seed_context = build_seed_context(paths, scenario)
        agent.run_characterization_scenario(scenario_id, paths.repo, seed_context)
        ensure_characterization_scenario_completed(
            paths.characterization_db,
            scenario_id,
            paths.repo,
            scenario.get("scaffold_path"),
        )
        commit_characterization_test(
            runner,
            paths.repo,
            paths.characterization_db,
            scenario_id,
            scenario.get("scaffold_path"),
        )
        completed.append(scenario_id)
        agent.compact_characterization_context(paths.repo, scenario_id)

    return completed


def count_incomplete_scenarios(database: Path, repo_path: Path | None = None) -> int:
    return sum(
        1
        for scenario in candidate_scenarios(database)
        if not scenario_is_complete(database, scenario, repo_path)
    )


def candidate_scenarios(database: Path) -> list[dict[str, Any]]:
    with closing(sqlite3.connect(database)) as connection:
        with connection:
            connection.row_factory = sqlite3.Row
            rows = connection.execute(
                candidate_scenarios_sql(scenario_projection())
                + f"""
                    GROUP BY s.{SCENARIO_ID}
                    ORDER BY s.{SCENARIO_ID}
                  """
            ).fetchall()
    return [dict(row) for row in rows]


def select_next_scenario(
    paths: HarnessPaths,
    excluded_ids: set[str],
) -> dict[str, Any] | None:
    for row in candidate_scenarios(paths.characterization_db):
        if row["scenario_id"] not in excluded_ids and not scenario_is_complete(
            paths.characterization_db,
            row,
            paths.repo,
        ):
            return row
    return None


def candidate_scenarios_sql(select_clause: str) -> str:
    return f"""
        SELECT {select_clause}
        FROM {SCENARIOS_TABLE} s
        JOIN {BEHAVIORS_TABLE} b ON b.{BEHAVIOR_ID} = s.{SCENARIO_BEHAVIOR_ID}
        LEFT JOIN {FILES_TABLE} f ON f.{FILE_SCENARIO_ID} = s.{SCENARIO_ID}
        WHERE s.{SCENARIO_STATUS} != '{SKIPPED_SCENARIO_STATUS}'
    """


def scenario_is_complete(
    database: Path,
    scenario: dict[str, Any],
    repo_path: Path | None,
) -> bool:
    status = str(scenario["status"])
    if status == SKIPPED_SCENARIO_STATUS:
        return True
    if status not in {ACCEPTED_SCENARIO_STATUS, COMMITTED_SCENARIO_STATUS}:
        return False
    return scenario_has_input_output_coverage(
        database,
        str(scenario["scenario_id"]),
        repo_path,
        scenario.get("scaffold_path"),
    )


def scenario_projection() -> str:
    return f"""
        s.{SCENARIO_ID} AS scenario_id,
        s.{SCENARIO_BEHAVIOR_ID},
        b.{BEHAVIOR_KG_NODE_ID},
        s.{SCENARIO_NAME},
        s.{SCENARIO_KIND},
        s.{SCENARIO_INVOCATION_KIND},
        s.{SCENARIO_STATUS},
        s.{SCENARIO_DIAGNOSTIC_REASON},
        MIN(f.{FILE_PATH}) AS scaffold_path
    """


def build_seed_context(paths: HarnessPaths, scenario: dict[str, Any]) -> dict[str, Any]:
    return {
        "scenario_id": scenario["scenario_id"],
        "behavior_id": scenario["behavior_id"],
        "kg_node_id": scenario["kg_node_id"],
        "abstract_scaffold_path": scenario.get("scaffold_path"),
        "database_paths": {
            "extraction": str(paths.extraction_db),
            "business_kg": str(paths.business_kg_db),
            "characterization_tests": str(paths.characterization_db),
        },
        "repo_path": str(paths.repo),
        "allowed_commands_tools": [
            "git status",
            "git diff",
            "project-local build/test commands",
            "jdtls from PATH",
            "gluon-cli code-parser db tables/schema/rows/insert/update",
        ],
        "relevant_status_rows": {
            "scenario": {
                "id": scenario["scenario_id"],
                "status": scenario["status"],
                "diagnostic_reason": scenario["diagnostic_reason"],
                "name": scenario["name"],
                "scenario_kind": scenario["scenario_kind"],
                "invocation_kind": scenario["invocation_kind"],
            }
        },
    }


def ensure_characterization_scenario_completed(
    database: Path,
    scenario_id: str,
    repo_path: Path | None = None,
    scaffold_path: str | None = None,
) -> None:
    with closing(sqlite3.connect(database)) as connection:
        with connection:
            row = connection.execute(
                f"""
                SELECT
                    s.{SCENARIO_STATUS},
                    COUNT(DISTINCT i.rowid),
                    COUNT(DISTINCT o.rowid)
                FROM {SCENARIOS_TABLE} s
                LEFT JOIN {INPUTS_TABLE} i ON i.{INPUT_SCENARIO_ID} = s.{SCENARIO_ID}
                LEFT JOIN {OBSERVATIONS_TABLE} o
                    ON o.{OBSERVATION_SCENARIO_ID} = s.{SCENARIO_ID}
                WHERE s.{SCENARIO_ID} = ?
                GROUP BY s.{SCENARIO_ID}, s.{SCENARIO_STATUS}
                """,
                [scenario_id],
            ).fetchone()

    if row is None:
        raise StageFailedError(f"missing characterization scenario {scenario_id}")
    status, input_count, observation_count = row
    if status not in COMPLETED_SCENARIO_STATUSES:
        raise StageFailedError(
            f"characterization scenario {scenario_id} not accepted: {status}"
        )
    if input_count == 0 or observation_count == 0:
        raise StageFailedError(
            "characterization scenario "
            f"{scenario_id} missing stored inputs or observations"
        )
    if not scenario_has_input_output_coverage(
        database,
        scenario_id,
        repo_path,
        scaffold_path,
    ):
        missing = missing_input_output_methods(database, scenario_id, repo_path, scaffold_path)
        raise StageFailedError(
            "characterization scenario "
            f"{scenario_id} missing input/output coverage for test methods: "
            f"{', '.join(missing)}"
        )


def scenario_has_input_output_coverage(
    database: Path,
    scenario_id: str,
    repo_path: Path | None,
    scaffold_path: str | None,
) -> bool:
    return not missing_input_output_methods(database, scenario_id, repo_path, scaffold_path)


def missing_input_output_methods(
    database: Path,
    scenario_id: str,
    repo_path: Path | None,
    scaffold_path: str | None,
) -> list[str]:
    test_methods = test_methods_for_scaffold(repo_path, scaffold_path)
    if not test_methods:
        return []
    covered_methods = input_methods_with_observations(database, scenario_id)
    return sorted(test_methods - covered_methods)


def test_methods_for_scaffold(repo_path: Path | None, scaffold_path: str | None) -> set[str]:
    if repo_path is None or not scaffold_path:
        return set()
    test_file = repo_path / scaffold_path
    if not test_file.exists():
        raise StageFailedError(f"missing characterization test file {scaffold_path}")
    source = test_file.read_text(encoding="utf-8")
    return set(
        re.findall(
            r"@Test(?:\s*\([^)]*\))?(?:\s*@\w+(?:\([^)]*\))?)*\s*(?:public\s+)?void\s+([A-Za-z_]\w*)\s*\(",
            source,
        )
    )


def input_methods_with_observations(database: Path, scenario_id: str) -> set[str]:
    with closing(sqlite3.connect(database)) as connection:
        with connection:
            rows = connection.execute(
                f"""
                SELECT i.{INPUT_JSON}, COUNT(o.rowid)
                FROM {INPUTS_TABLE} i
                LEFT JOIN {OBSERVATIONS_TABLE} o
                    ON o.{OBSERVATION_INPUT_ID} = i.{INPUT_ID}
                    AND o.{OBSERVATION_SCENARIO_ID} = i.{INPUT_SCENARIO_ID}
                WHERE i.{INPUT_SCENARIO_ID} = ?
                GROUP BY i.{INPUT_ID}, i.{INPUT_JSON}
                """,
                [scenario_id],
            ).fetchall()

    methods = set()
    for input_json, observation_count in rows:
        if observation_count == 0:
            continue
        try:
            method = json.loads(input_json).get("method")
        except json.JSONDecodeError:
            continue
        if isinstance(method, str) and method:
            methods.add(method)
    return methods


def commit_characterization_test(
    runner: CommandRunner,
    repo_path: Path,
    characterization_db: Path,
    scenario_id: str,
    scaffold_path: str | None,
) -> None:
    status = runner.run(["git", "status", "--short"], cwd=repo_path)
    if not status.stdout.strip():
        return

    add_paths = characterization_add_paths(repo_path, characterization_db, scaffold_path)
    add = runner.run(["git", "add", *add_paths], cwd=repo_path)
    if not add.ok:
        raise StageFailedError(f"git add failed for characterization {scenario_id}")
    commit = runner.run(
        ["git", "commit", "-m", f"Add characterization test for {scenario_id}"],
        cwd=repo_path,
    )
    if not commit.ok:
        raise StageFailedError(f"git commit failed for characterization {scenario_id}")


def characterization_add_paths(
    repo_path: Path,
    characterization_db: Path,
    scaffold_path: str | None,
) -> list[str]:
    add_paths = [scaffold_path] if scaffold_path else ["gluon/tests"]
    try:
        add_paths.append(str(characterization_db.relative_to(repo_path)))
    except ValueError:
        pass
    return add_paths
