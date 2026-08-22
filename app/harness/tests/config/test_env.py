import unittest

from config.env import load_config
from errors import ConfigError


VALID_ENV = {
    "BACKEND_URL": "mock://local",
    "LANGUAGE": "java",
    "CURRENT_VERSION": "9",
    "TARGET_VERSION": "25",
    "ORG_PROJECT_NAME": "org/project",
    "ANTHROPIC_API_KEY": "key",
    "ANTHROPIC_MODEL": "model",
    "ANTHROPIC_BASE_URL": "https://example.test",
}


class EnvConfigTests(unittest.TestCase):
    def test_loads_valid_config(self) -> None:
        config = load_config(VALID_ENV)

        self.assertEqual(config.language, "java")
        self.assertEqual(config.max_agent_attempts, 3)

    def test_rejects_non_java_language(self) -> None:
        env = dict(VALID_ENV, LANGUAGE="python")

        with self.assertRaises(ConfigError):
            load_config(env)

    def test_rejects_invalid_max_attempts(self) -> None:
        env = dict(VALID_ENV, MAX_AGENT_ATTEMPTS="0")

        with self.assertRaises(ConfigError):
            load_config(env)


if __name__ == "__main__":
    unittest.main()

