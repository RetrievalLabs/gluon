from contextlib import closing
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
OBSERVATION_SCENARIO_ID = characterization_field(
    characterization_tests_pb2.CharacterizationObservationRow,
    "scenario_id",
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
    for _ in range(count_incomplete_scenarios(paths.characterization_db)):
        scenario = select_next_scenario(paths, processed)
        if scenario is None:
            break

        scenario_id = str(scenario["scenario_id"])
        processed.add(scenario_id)
        seed_context = build_seed_context(paths, scenario)
        agent.run_characterization_scenario(scenario_id, paths.repo, seed_context)
        ensure_characterization_scenario_completed(paths.characterization_db, scenario_id)
        commit_characterization_test(
            runner,
            paths.repo,
            paths.characterization_db,
            scenario_id,
            scenario.get("scaffold_path"),
        )
        completed.append(scenario_id)

    return completed


def count_incomplete_scenarios(database: Path) -> int:
    with closing(sqlite3.connect(database)) as connection:
        with connection:
            return int(
                connection.execute(
                    incomplete_scenarios_sql("COUNT(DISTINCT s.id)"),
                ).fetchone()[0]
            )


def select_next_scenario(
    paths: HarnessPaths,
    excluded_ids: set[str],
) -> dict[str, Any] | None:
    with closing(sqlite3.connect(paths.characterization_db)) as connection:
        with connection:
            connection.row_factory = sqlite3.Row
            rows = connection.execute(
                incomplete_scenarios_sql(
                    scenario_projection()
                )
                + f"""
                    GROUP BY s.{SCENARIO_ID}
                    ORDER BY s.{SCENARIO_ID}
                  """
            ).fetchall()

    for row in rows:
        if row["scenario_id"] not in excluded_ids:
            return dict(row)
    return None


def incomplete_scenarios_sql(select_clause: str) -> str:
    return f"""
        SELECT {select_clause}
        FROM {SCENARIOS_TABLE} s
        JOIN {BEHAVIORS_TABLE} b ON b.{BEHAVIOR_ID} = s.{SCENARIO_BEHAVIOR_ID}
        LEFT JOIN {FILES_TABLE} f ON f.{FILE_SCENARIO_ID} = s.{SCENARIO_ID}
        LEFT JOIN {INPUTS_TABLE} i ON i.{INPUT_SCENARIO_ID} = s.{SCENARIO_ID}
        LEFT JOIN {OBSERVATIONS_TABLE} o ON o.{OBSERVATION_SCENARIO_ID} = s.{SCENARIO_ID}
        WHERE NOT {completed_scenario_sql()}
    """


def completed_scenario_sql() -> str:
    return f"""
        (
            s.{SCENARIO_STATUS} = '{SKIPPED_SCENARIO_STATUS}'
            OR (
                s.{SCENARIO_STATUS} IN (
                    '{ACCEPTED_SCENARIO_STATUS}',
                    '{COMMITTED_SCENARIO_STATUS}'
                )
                AND i.rowid IS NOT NULL
                AND o.rowid IS NOT NULL
            )
        )
    """


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


def ensure_characterization_scenario_completed(database: Path, scenario_id: str) -> None:
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
