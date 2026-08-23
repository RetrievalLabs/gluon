from generated.gluon.db.v1 import business_kg_pb2
from generated.gluon.db.v1 import characterization_tests_pb2
from generated.gluon.db.v1 import extraction_pb2


def characterization_table(table: int) -> str:
    return enum_db_value(
        characterization_tests_pb2.CharacterizationTable.Name(table),
        "CHARACTERIZATION_TABLE_",
        "characterization_",
    )


def characterization_scenario_status(status: int) -> str:
    return enum_db_value(
        characterization_tests_pb2.CharacterizationScenarioStatus.Name(status),
        "CHARACTERIZATION_SCENARIO_STATUS_",
    )


def business_kg_table(table: int) -> str:
    return enum_db_value(
        business_kg_pb2.BusinessKgTable.Name(table),
        "BUSINESS_KG_TABLE_",
    )


def extraction_table(table: int) -> str:
    return enum_db_value(
        extraction_pb2.ExtractionTable.Name(table),
        "EXTRACTION_TABLE_",
    )


def enum_db_value(enum_name: str, prefix: str, table_prefix: str = "") -> str:
    if not enum_name.startswith(prefix):
        raise ValueError(f"unexpected enum name {enum_name}")
    return f"{table_prefix}{enum_name.removeprefix(prefix).lower()}"

