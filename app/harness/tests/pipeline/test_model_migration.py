import json
import tempfile
import unittest
from pathlib import Path

from errors import StageFailedError
from execution.paths import HarnessPaths
from models.agent import AgentAttempt
from models.command import CommandResult
from pipeline.model_migration import (
    load_model_migration_tasks,
    pending_model_migration_tasks,
    run_model_migration_agent_loop,
    skills_for_task,
)


VALID_AGENT_JSON = (
    '{"status":"completed","changed_files":["src/main/java/demo/Order.java"],'
    '"verification":[{"command":"mvn -pl service test","status":"passed"}],'
    '"blockers":[]}'
)


class FakeAgent:
    def __init__(self, message: str = VALID_AGENT_JSON, write_file: bool = True) -> None:
        self.message = message
        self.write_file = write_file
        self.calls = []
        self.compactions = []

    def run_model_migration(
        self,
        rewrite_workspace,
        legacy_repo_path,
        build_report_path,
        compatibility_report_path,
        model_classification_report_path,
        dependency_selection_report_path,
        build_structure_report_path,
        extraction_db_path,
        business_kg_db_path,
        characterization_db_path,
        target_version,
        task_context,
        skills,
    ):
        self.calls.append((task_context, skills))
        if self.write_file:
            expected = Path(task_context["expected_rewrite_file"])
            expected.parent.mkdir(parents=True, exist_ok=True)
            expected.write_text("package demo;\nclass Order {}\n", encoding="utf-8")
        return AgentAttempt("model-migration", 1, "completed", self.message)

    def compact_model_migration_context(self, rewrite_workspace, task_id, result_json):
        self.compactions.append((rewrite_workspace, task_id, result_json))


class FakeRunner:
    def __init__(self) -> None:
        self.commands = []

    def run(self, command, cwd=None, env=None):
        self.commands.append((command, cwd))
        stdout = " M src/main/java/demo/Order.java\n" if command == ["git", "status", "--short"] else ""
        return CommandResult(command, str(cwd) if cwd else None, 0, stdout, "", 1)


class ModelMigrationTests(unittest.TestCase):
    def test_flattens_nested_report_in_stable_order(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            report = Path(directory) / "model-classification-report.json"
            report.write_text(json.dumps(sample_report()), encoding="utf-8")

            tasks = load_model_migration_tasks(report)

        self.assertEqual(
            [task.name for task in tasks],
            [
                "demo.CreateOrderRequest",
                "demo.Order",
                "demo.OrderRepository",
                "demo.OrderResponse",
            ],
        )
        self.assertEqual(tasks[0].row_type, "dtos")
        self.assertEqual(tasks[0].module_path, "service")

    def test_skips_model_when_same_path_exists_in_rewrite_workspace(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            paths = HarnessPaths.from_org_project("org/project", Path(directory))
            paths.model_classification_report.parent.mkdir(parents=True)
            paths.model_classification_report.write_text(
                json.dumps(sample_report()),
                encoding="utf-8",
            )
            existing = paths.rewrite_workspace / "src/main/java/demo/Order.java"
            existing.parent.mkdir(parents=True)
            existing.write_text("package demo;\nclass Order {}\n", encoding="utf-8")

            tasks = pending_model_migration_tasks(paths)

        self.assertNotIn("demo.Order", [task.name for task in tasks])
        self.assertIn("demo.CreateOrderRequest", [task.name for task in tasks])

    def test_derives_dependency_specific_skills(self) -> None:
        task = load_model_migration_tasks_from_sample()[1]

        skills = skills_for_task(task)

        self.assertIn("version-rewrite-modernization", skills)
        self.assertIn("java-best-practices", skills)
        self.assertIn("java-lombok-modernization", skills)
        self.assertIn("spring-boot-best-practices", skills)
        self.assertIn("database-orm-best-practices", skills)
        self.assertNotIn("jakarta-ee-best-practices", skills)

    def test_runs_agent_commits_and_compacts_completed_model(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            paths = HarnessPaths.from_org_project("org/project", Path(directory))
            paths.model_classification_report.parent.mkdir(parents=True)
            paths.model_classification_report.write_text(
                json.dumps(one_model_report()),
                encoding="utf-8",
            )
            agent = FakeAgent()
            runner = FakeRunner()

            completed = run_model_migration_agent_loop(paths, agent, runner, "25")

        self.assertEqual(len(completed), 1)
        self.assertEqual(len(agent.calls), 1)
        self.assertEqual(runner.commands[-1][0], ["git", "commit", "-m", "Migrate model demo.Order"])
        self.assertEqual(len(agent.compactions), 1)
        self.assertIn('"status":"completed"', agent.compactions[0][2])

    def test_fails_when_agent_blocks(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            paths = HarnessPaths.from_org_project("org/project", Path(directory))
            paths.model_classification_report.parent.mkdir(parents=True)
            paths.model_classification_report.write_text(
                json.dumps(one_model_report()),
                encoding="utf-8",
            )
            agent = FakeAgent(
                '{"status":"blocked","changed_files":[],"verification":[],"blockers":["missing serializer context"]}'
            )

            with self.assertRaises(StageFailedError):
                run_model_migration_agent_loop(paths, agent, FakeRunner(), "25")

    def test_fails_when_expected_source_is_missing(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            paths = HarnessPaths.from_org_project("org/project", Path(directory))
            paths.model_classification_report.parent.mkdir(parents=True)
            paths.model_classification_report.write_text(
                json.dumps(one_model_report()),
                encoding="utf-8",
            )

            with self.assertRaises(StageFailedError):
                run_model_migration_agent_loop(
                    paths,
                    FakeAgent(write_file=False),
                    FakeRunner(),
                    "25",
                )


def load_model_migration_tasks_from_sample():
    with tempfile.TemporaryDirectory() as directory:
        report = Path(directory) / "model-classification-report.json"
        report.write_text(json.dumps(sample_report()), encoding="utf-8")
        return load_model_migration_tasks(report)


def one_model_report():
    report = sample_report()
    report["modules"][0]["children"][0]["dtos"] = []
    report["modules"][0]["children"][0]["response_bodies"] = []
    report["modules"][0]["children"][0]["repositories"] = []
    return report


def sample_report():
    return {
        "modules": [
            {
                "id": "root",
                "name": "root",
                "path": ".",
                "children": [
                    {
                        "id": "service",
                        "name": "service",
                        "path": "service",
                        "used_dependencies": [
                            {
                                "group_id": "org.springframework.boot",
                                "artifact_id": "spring-boot-starter-data-jpa",
                                "version": "3.3.0",
                            },
                            {
                                "group_id": "jakarta.persistence",
                                "artifact_id": "jakarta.persistence-api",
                                "version": "3.1.0",
                            },
                        ],
                        "models": [],
                        "dtos": [
                            {
                                "qualified_name": "demo.CreateOrderRequest",
                                "kind": "record",
                                "module_id": "service",
                                "file": "src/main/java/demo/CreateOrderRequest.java",
                                "start_line": 1,
                                "end_line": 1,
                                "classification": "request_body",
                                "evidence": [],
                            }
                        ],
                        "request_bodies": [],
                        "response_bodies": [
                            {
                                "type_name": "demo.OrderResponse",
                                "owner": "demo.OrderController",
                                "method": "create",
                                "module_id": "service",
                                "file": "src/main/java/demo/OrderResponse.java",
                                "line": 2,
                                "evidence": [],
                            }
                        ],
                        "entities": [
                            {
                                "qualified_name": "demo.Order",
                                "module_id": "service",
                                "file": "src/main/java/demo/Order.java",
                                "start_line": 3,
                                "end_line": 10,
                                "table_name": "orders",
                                "evidence": [],
                            }
                        ],
                        "repositories": [
                            {
                                "qualified_name": "demo.OrderRepository",
                                "module_id": "service",
                                "file": "src/main/java/demo/OrderRepository.java",
                                "start_line": 4,
                                "end_line": 4,
                                "entity_type": "demo.Order",
                                "query_methods": [],
                                "evidence": [],
                            }
                        ],
                        "tables": [],
                        "columns": [],
                        "children": [],
                    }
                ],
            }
        ]
    }


if __name__ == "__main__":
    unittest.main()
