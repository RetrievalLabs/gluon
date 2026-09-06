from integrations.gluon_cli import GluonCli
from models.config import HarnessConfig
from models.stage import Stage


def build_stages(config: HarnessConfig, gluon: GluonCli) -> list[Stage]:
    cwd = str(gluon.paths.repo)
    return [
        Stage("parse-build", gluon.parse_build(), cwd),
        Stage("analyze-report", gluon.analyze_report(config.target_version), cwd),
        Stage("classify-models", gluon.classify_models(), cwd),
        Stage("classify-configs", gluon.classify_configs(), cwd),
        Stage("extract-business", gluon.extract_business(), cwd),
        Stage("extract-tests", gluon.extract_tests(), cwd),
        Stage("build-business-kg", gluon.build_business_kg(), cwd),
        Stage(
            "generate-characterization-tests",
            gluon.generate_characterization_tests(),
            cwd,
        ),
    ]
