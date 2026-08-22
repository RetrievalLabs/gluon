import json
import tempfile
import unittest
from pathlib import Path

from execution.commands import CommandRunner


class CommandRunnerTests(unittest.TestCase):
    def test_records_command_result(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            log_path = Path(directory) / "commands.jsonl"

            result = CommandRunner(log_path).run(["/bin/sh", "-c", "printf ok"])

            self.assertTrue(result.ok)
            record = json.loads(log_path.read_text(encoding="utf-8"))
            self.assertEqual(record["command"], ["/bin/sh", "-c", "printf ok"])
            self.assertEqual(record["stdout"], "ok")


if __name__ == "__main__":
    unittest.main()

