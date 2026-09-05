import unittest

from models.agent import parse_agent_json_response


class AgentJsonResponseTests(unittest.TestCase):
    def test_parses_completed_response(self) -> None:
        response = parse_agent_json_response(
            '{"status":"completed","changed_files":["pom.xml"],'
            '"verification":[{"command":"mvn test","status":"passed"}],'
            '"blockers":[]}'
        )

        self.assertEqual(response.status, "completed")
        self.assertEqual(response.changed_files, ["pom.xml"])
        self.assertEqual(response.verification[0].command, "mvn test")
        self.assertEqual(response.verification[0].status, "passed")
        self.assertEqual(response.blockers, [])

    def test_parses_blocked_response(self) -> None:
        response = parse_agent_json_response(
            '{"status":"blocked","changed_files":[],"verification":[],'
            '"blockers":["missing build file"]}'
        )

        self.assertEqual(response.status, "blocked")
        self.assertEqual(response.blockers, ["missing build file"])

    def test_rejects_non_json_text(self) -> None:
        with self.assertRaises(ValueError):
            parse_agent_json_response("done")

    def test_rejects_markdown_fence(self) -> None:
        with self.assertRaises(ValueError):
            parse_agent_json_response(
                '```json\n{"status":"completed","changed_files":[],'
                '"verification":[],"blockers":[]}\n```'
            )

    def test_rejects_bad_status(self) -> None:
        with self.assertRaises(ValueError):
            parse_agent_json_response(
                '{"status":"done","changed_files":[],"verification":[],'
                '"blockers":[]}'
            )

    def test_rejects_missing_fields(self) -> None:
        with self.assertRaises(ValueError):
            parse_agent_json_response('{"status":"completed"}')

    def test_rejects_wrong_field_types(self) -> None:
        with self.assertRaises(ValueError):
            parse_agent_json_response(
                '{"status":"completed","changed_files":"pom.xml",'
                '"verification":[],"blockers":[]}'
            )

    def test_rejects_trailing_text(self) -> None:
        with self.assertRaises(ValueError):
            parse_agent_json_response(
                '{"status":"completed","changed_files":[],"verification":[],'
                '"blockers":[]} done'
            )

    def test_rejects_blocked_without_blocker(self) -> None:
        with self.assertRaises(ValueError):
            parse_agent_json_response(
                '{"status":"blocked","changed_files":[],"verification":[],'
                '"blockers":[]}'
            )


if __name__ == "__main__":
    unittest.main()
