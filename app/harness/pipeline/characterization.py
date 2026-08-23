from contextlib import closing
import sqlite3
from pathlib import Path
from typing import Any

from db_contracts import characterization_scenario_status, characterization_table
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
        commit_characterization_test(runner, paths.repo, scenario_id)
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
                    """
                    s.id AS scenario_id,
                    s.behavior_id,
                    b.kg_node_id,
                    s.name,
                    s.scenario_kind,
                    s.invocation_kind,
                    s.status,
                    s.diagnostic_reason,
                    MIN(f.path) AS scaffold_path
                    """
                )
                + """
                    GROUP BY s.id
                    ORDER BY s.id
                  """,
            ).fetchall()

    for row in rows:
        if row["scenario_id"] not in excluded_ids:
            return dict(row)
    return None


def incomplete_scenarios_sql(select_clause: str) -> str:
    placeholders = ", ".join(f"'{status}'" for status in COMPLETED_SCENARIO_STATUSES)
    return f"""
        SELECT {select_clause}
        FROM {SCENARIOS_TABLE} s
        JOIN {BEHAVIORS_TABLE} b ON b.id = s.behavior_id
        LEFT JOIN {FILES_TABLE} f ON f.scenario_id = s.id
        WHERE s.status NOT IN ({placeholders})
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
            "gluon-cli code-parser db tables/schema/rows/update",
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


def commit_characterization_test(
    runner: CommandRunner,
    repo_path: Path,
    scenario_id: str,
) -> None:
    status = runner.run(["git", "status", "--short"], cwd=repo_path)
    if not status.stdout.strip():
        return

    add = runner.run(["git", "add", "gluon/tests"], cwd=repo_path)
    if not add.ok:
        raise StageFailedError(f"git add failed for characterization {scenario_id}")
    commit = runner.run(
        ["git", "commit", "-m", f"Add characterization test for {scenario_id}"],
        cwd=repo_path,
    )
    if not commit.ok:
        raise StageFailedError(f"git commit failed for characterization {scenario_id}")
