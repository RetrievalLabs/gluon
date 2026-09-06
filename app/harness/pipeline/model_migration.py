import json
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from errors import StageFailedError
from execution.commands import CommandRunner
from execution.paths import HarnessPaths
from integrations.claude_agent import ClaudeAgentClient
from models.agent import parse_agent_json_response
from pipeline.migration_commit import commit_rewrite_workspace


MODEL_ROW_TYPES = (
    "models",
    "dtos",
    "request_bodies",
    "response_bodies",
    "entities",
    "repositories",
)


@dataclass(frozen=True)
class ModelMigrationTask:
    task_id: str
    row_type: str
    name: str
    module_path: str
    source_file: str
    start_line: int
    row: dict[str, Any]
    module: dict[str, Any]
    used_dependencies: list[dict[str, Any]]

    @property
    def expected_destination(self) -> str:
        return self.source_file


def run_model_migration_agent_loop(
    paths: HarnessPaths,
    agent: ClaudeAgentClient,
    runner: CommandRunner,
    target_version: str,
) -> list[str]:
    tasks = pending_model_migration_tasks(paths)
    completed: list[str] = []
    for task in tasks:
        if (paths.rewrite_workspace / task.expected_destination).exists():
            continue
        result = agent.run_model_migration(
            paths.rewrite_workspace,
            paths.repo,
            paths.build_report,
            paths.compatibility_report,
            paths.model_classification_report,
            paths.dependency_selection_report,
            paths.build_structure_report,
            paths.extraction_db,
            paths.business_kg_db,
            paths.characterization_db if paths.characterization_db.exists() else None,
            target_version,
            task_context(paths, task),
            skills_for_task(task),
        )
        response = parse_agent_json_response(result.message)
        if response.status == "blocked":
            raise StageFailedError(
                "model migration blocked for "
                f"{task.task_id}: {', '.join(response.blockers)}"
            )
        ensure_model_migrated(paths, task)
        commit_rewrite_workspace(
            runner,
            paths.rewrite_workspace,
            f"Migrate model {task.name}",
        )
        completed.append(task.task_id)
        agent.compact_model_migration_context(
            paths.rewrite_workspace,
            task.task_id,
            response.to_json(),
        )
    return completed


def pending_model_migration_tasks(paths: HarnessPaths) -> list[ModelMigrationTask]:
    return [
        task
        for task in load_model_migration_tasks(paths.model_classification_report)
        if not (paths.rewrite_workspace / task.expected_destination).exists()
    ]


def load_model_migration_tasks(report_path: Path) -> list[ModelMigrationTask]:
    if not report_path.exists():
        return []
    report = json.loads(report_path.read_text(encoding="utf-8"))
    tasks: list[ModelMigrationTask] = []
    for module in flatten_modules(report.get("modules", [])):
        for row_type in MODEL_ROW_TYPES:
            for row in module.get(row_type, []):
                source_file = model_source_file(row)
                if not source_file:
                    continue
                name = model_name(row)
                start_line = int(row.get("start_line") or row.get("line") or 0)
                tasks.append(
                    ModelMigrationTask(
                        task_id=task_id(module, row_type, name, source_file, start_line),
                        row_type=row_type,
                        name=name,
                        module_path=str(module.get("path") or ""),
                        source_file=source_file,
                        start_line=start_line,
                        row=row,
                        module=module_summary(module),
                        used_dependencies=list(module.get("used_dependencies", [])),
                    )
                )
    return sorted(
        tasks,
        key=lambda task: (
            task.module_path,
            task.source_file,
            task.start_line,
            task.row_type,
            task.name,
        ),
    )


def flatten_modules(modules: list[dict[str, Any]]) -> list[dict[str, Any]]:
    flattened: list[dict[str, Any]] = []
    for module in modules:
        flattened.append(module)
        flattened.extend(flatten_modules(module.get("children", [])))
    return flattened


def model_source_file(row: dict[str, Any]) -> str:
    return str(row.get("file") or "")


def model_name(row: dict[str, Any]) -> str:
    for key in ("qualified_name", "type_name", "entity"):
        value = row.get(key)
        if value:
            return str(value)
    owner = row.get("owner")
    method = row.get("method")
    if owner and method:
        return f"{owner}.{method}"
    return "unknown"


def task_id(
    module: dict[str, Any],
    row_type: str,
    name: str,
    source_file: str,
    start_line: int,
) -> str:
    module_id = str(module.get("id") or module.get("path") or "root")
    return f"{module_id}:{row_type}:{name}:{source_file}:{start_line}"


def module_summary(module: dict[str, Any]) -> dict[str, Any]:
    return {
        "id": module.get("id"),
        "name": module.get("name"),
        "path": module.get("path"),
        "used_dependencies": module.get("used_dependencies", []),
    }


def task_context(paths: HarnessPaths, task: ModelMigrationTask) -> dict[str, Any]:
    return {
        "task_id": task.task_id,
        "row_type": task.row_type,
        "name": task.name,
        "module": task.module,
        "selected_row": task.row,
        "used_dependencies": task.used_dependencies,
        "source_file": str(paths.repo / task.source_file),
        "expected_rewrite_file": str(paths.rewrite_workspace / task.expected_destination),
        "database_paths": {
            "extraction": str(paths.extraction_db),
            "business_kg": str(paths.business_kg_db),
            "characterization_tests": (
                str(paths.characterization_db)
                if paths.characterization_db.exists()
                else None
            ),
        },
    }


def skills_for_task(task: ModelMigrationTask) -> list[str]:
    skills = [
        "version-rewrite-modernization",
        "java-best-practices",
        "java-lombok-modernization",
    ]
    dependency_text = json.dumps(task.used_dependencies).lower()
    row_type = task.row_type
    if "spring-boot" in dependency_text:
        skills.append("spring-boot-best-practices")
    if (
        "spring-web" in dependency_text
        or "spring-mvc" in dependency_text
        or "starter-web" in dependency_text
        or row_type in {"request_bodies", "response_bodies"}
    ):
        skills.append("spring-mvc-best-practices")
    if "spring-security" in dependency_text or "starter-security" in dependency_text:
        skills.append("spring-security-best-practices")
    if (
        "hibernate" in dependency_text
        or "spring-data-jpa" in dependency_text
        or "starter-data-jpa" in dependency_text
        or "data-jpa" in dependency_text
        or "jakarta.persistence" in dependency_text
        or "javax.persistence" in dependency_text
        or row_type in {"entities", "repositories"}
    ):
        skills.append("database-orm-best-practices")
    if "junit" in dependency_text or "mockito" in dependency_text:
        skills.append("junit-mockito-testing-best-practices")
    if "jakarta" in dependency_text and "spring-boot" not in dependency_text:
        skills.append("jakarta-ee-best-practices")
    return dedupe(skills)


def dedupe(values: list[str]) -> list[str]:
    seen: set[str] = set()
    result: list[str] = []
    for value in values:
        if value not in seen:
            seen.add(value)
            result.append(value)
    return result


def ensure_model_migrated(paths: HarnessPaths, task: ModelMigrationTask) -> None:
    expected = paths.rewrite_workspace / task.expected_destination
    if not expected.exists():
        raise StageFailedError(
            "model migration did not write expected source file "
            f"{expected}"
        )
