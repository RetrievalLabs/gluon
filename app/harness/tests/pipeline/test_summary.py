import json
import tempfile
import unittest
from pathlib import Path

from models.summary import RunSummary
from pipeline.summary import write_summary


class SummaryTests(unittest.TestCase):
    def test_writes_summary_json(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "summary.json"

            write_summary(path, RunSummary(status="ok", completed_stages=["parse-build"]))

            payload = json.loads(path.read_text(encoding="utf-8"))
            self.assertEqual(payload["status"], "ok")
            self.assertEqual(payload["completed_stages"], ["parse-build"])


if __name__ == "__main__":
    unittest.main()

