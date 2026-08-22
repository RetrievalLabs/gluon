import unittest

from integrations.backend import BackendClient
from models.config import HarnessConfig


class BackendTests(unittest.TestCase):
    def test_mock_backend_returns_repo_info(self) -> None:
        config = HarnessConfig(
            backend_url="mock://local",
            language="java",
            current_version="9",
            target_version="25",
            org_project_name="org/project",
            anthropic_api_key="key",
            anthropic_model="model",
            anthropic_base_url="base",
        )
        env = {"MOCK_REPO_URL": "https://repo.test/project", "MOCK_SOURCE_BRANCH": "main"}

        repo = BackendClient(config, env).fetch_repo()

        self.assertEqual(repo.repo_url, "https://repo.test/project")
        self.assertEqual(repo.source_branch, "main")


if __name__ == "__main__":
    unittest.main()

