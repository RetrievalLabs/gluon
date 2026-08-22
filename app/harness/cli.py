import os
import sys

from config.env import load_config
from errors import HarnessError
from pipeline.coordinator import PipelineCoordinator


def main() -> int:
    try:
        config = load_config()
        PipelineCoordinator(config, dict(os.environ)).run()
    except HarnessError as error:
        print(f"gluon-harness: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
