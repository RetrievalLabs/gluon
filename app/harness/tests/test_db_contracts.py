import unittest

from db_contracts import (
    business_kg_table,
    characterization_scenario_status,
    characterization_table,
    extraction_table,
)
from generated.gluon.db.v1 import business_kg_pb2
from generated.gluon.db.v1 import characterization_tests_pb2
from generated.gluon.db.v1 import extraction_pb2


class DbContractsTests(unittest.TestCase):
    def test_derives_sqlite_names_from_proto_clients(self) -> None:
        self.assertEqual(
            characterization_table(
                characterization_tests_pb2.CHARACTERIZATION_TABLE_SCENARIOS
            ),
            "characterization_scenarios",
        )
        self.assertEqual(
            characterization_scenario_status(
                characterization_tests_pb2
                .CHARACTERIZATION_SCENARIO_STATUS_GENERATED_SCAFFOLD
            ),
            "generated_scaffold",
        )
        self.assertEqual(
            business_kg_table(business_kg_pb2.BUSINESS_KG_TABLE_BUSINESS_NODES),
            "business_nodes",
        )
        self.assertEqual(
            extraction_table(extraction_pb2.EXTRACTION_TABLE_TEST_CASES),
            "test_cases",
        )


if __name__ == "__main__":
    unittest.main()
