from dataclasses import dataclass
from pathlib import Path


def project_slug(org_project_name: str) -> str:
    slug = org_project_name.rstrip("/").split("/")[-1].strip()
    return slug or "project"


@dataclass(frozen=True)
class HarnessPaths:
    root: Path
    project: str

    @classmethod
    def from_org_project(
        cls,
        org_project_name: str,
        root: Path = Path("/opt/gluon/org"),
    ) -> "HarnessPaths":
        return cls(root=root, project=project_slug(org_project_name))

    @property
    def repo(self) -> Path:
        return self.root / "project" / self.project

    @property
    def rewrite_workspace(self) -> Path:
        return self.root / "rewrite" / self.project

    @property
    def rewrite_docs_dir(self) -> Path:
        return self.rewrite_workspace / "docs"

    @property
    def legacy_tree(self) -> Path:
        return self.rewrite_docs_dir / "legacy-tree.txt"

    @property
    def build_report(self) -> Path:
        return self.build_report_output_dir / self.project / "build-report.json"

    @property
    def build_report_output_dir(self) -> Path:
        return self.root / "build-report"

    @property
    def compatibility_report(self) -> Path:
        return (
            self.compatibility_report_output_dir
            / self.project
            / "compatibility-report.json"
        )

    @property
    def compatibility_report_output_dir(self) -> Path:
        return self.root / "compatibility-report"

    @property
    def extraction_db(self) -> Path:
        return self.root / "extraction.db"

    @property
    def business_kg_db(self) -> Path:
        return self.root / "business-kg.db"

    @property
    def characterization_output_dir(self) -> Path:
        return self.repo / "gluon" / "tests"

    @property
    def characterization_db(self) -> Path:
        return self.characterization_output_dir / "characterization-tests.db"

    @property
    def command_log(self) -> Path:
        return self.root / "commands.jsonl"

    @property
    def agent_log(self) -> Path:
        return self.root / "agents.jsonl"

    @property
    def summary(self) -> Path:
        return self.root / "summary.json"
