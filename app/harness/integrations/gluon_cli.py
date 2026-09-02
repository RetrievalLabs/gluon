from execution.paths import HarnessPaths


class GluonCli:
    def __init__(self, executable: str, paths: HarnessPaths) -> None:
        self.executable = executable
        self.paths = paths

    def parse_build(self) -> list[str]:
        return [
            self.executable,
            "code-parser",
            "parse-build",
            "--path",
            str(self.paths.repo),
            "--resolve",
            "--format",
            "json",
            "--output-dir",
            str(self.paths.build_report_output_dir),
        ]

    def analyze_report(self, target_version: str) -> list[str]:
        return [
            self.executable,
            "code-parser",
            "analyze-report",
            "--report",
            str(self.paths.build_report),
            "--target-java",
            target_version,
            "--format",
            "json",
            "--source-path",
            str(self.paths.repo),
            "--output-dir",
            str(self.paths.compatibility_report_output_dir),
        ]

    def extract_business(self) -> list[str]:
        return [
            self.executable,
            "code-parser",
            "extract-business",
            "--path",
            str(self.paths.repo),
            "--output-dir",
            str(self.paths.root),
            "--database",
            str(self.paths.extraction_db),
            "--build-report",
            str(self.paths.build_report),
        ]

    def extract_tests(self) -> list[str]:
        return [
            self.executable,
            "code-parser",
            "extract-tests",
            "--path",
            str(self.paths.repo),
            "--database",
            str(self.paths.extraction_db),
        ]

    def build_business_kg(self) -> list[str]:
        return [
            self.executable,
            "code-parser",
            "build-business-kg",
            "--database",
            str(self.paths.extraction_db),
            "--source-path",
            str(self.paths.repo),
            "--output",
            str(self.paths.business_kg_db),
        ]

    def generate_characterization_tests(self) -> list[str]:
        return [
            self.executable,
            "code-parser",
            "generate-characterization-tests",
            "--business-database",
            str(self.paths.extraction_db),
            "--kg-database",
            str(self.paths.business_kg_db),
            "--source-path",
            str(self.paths.repo),
            "--output-dir",
            str(self.paths.characterization_output_dir),
        ]
