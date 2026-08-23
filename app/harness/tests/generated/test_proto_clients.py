import unittest

from generated.gluon.db.v1 import business_kg_pb2
from generated.gluon.db.v1 import characterization_tests_pb2
from generated.gluon.db.v1 import extraction_pb2


class ProtoClientTests(unittest.TestCase):
    def test_generated_database_contract_clients_import(self) -> None:
        scenario = characterization_tests_pb2.CharacterizationScenarioRow(
            id="scenario:one",
            behavior_id="behavior:one",
            name="Approve",
            scenario_kind=(
                characterization_tests_pb2
                .CHARACTERIZATION_SCENARIO_KIND_HAPPY_PATH
            ),
            status=(
                characterization_tests_pb2
                .CHARACTERIZATION_SCENARIO_STATUS_GENERATED_SCAFFOLD
            ),
        )
        method = extraction_pb2.MethodRow(id="method:one", name="approve")
        node = business_kg_pb2.BusinessNodeRow(id="node:one", name="Approve")

        self.assertEqual(scenario.id, "scenario:one")
        self.assertEqual(method.id, "method:one")
        self.assertEqual(node.id, "node:one")


if __name__ == "__main__":
    unittest.main()
