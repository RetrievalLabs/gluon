from collections.abc import Mapping

from errors import BackendError
from models.config import HarnessConfig, RepoInfo


class BackendClient:
    def __init__(
        self,
        config: HarnessConfig,
        env: Mapping[str, str],
    ) -> None:
        self.config = config
        self.env = env

    def fetch_repo(self) -> RepoInfo:
        if self.config.backend_url != "mock://local":
            raise BackendError("only BACKEND_URL=mock://local is implemented")
        repo_url = self.env.get("MOCK_REPO_URL")
        source_branch = self.env.get("MOCK_SOURCE_BRANCH")
        if not repo_url or not source_branch:
            raise BackendError(
                "MOCK_REPO_URL and MOCK_SOURCE_BRANCH are required for mock backend"
            )
        return RepoInfo(
            repo_url=repo_url,
            source_branch=source_branch,
            token=self.env.get("MOCK_REPO_TOKEN"),
        )

