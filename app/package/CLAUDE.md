# Package Contracts

`app/package/` contains protobuf contracts shared by Gluon components.

Scope:

- `extraction.db` row contracts used by code-parser and harness.
- `business-kg.db` row and lifecycle contracts used by code-parser and harness.
- `characterization-tests.db` row and lifecycle contracts used by code-parser
  and harness.
- Public inter-service API contracts when needed.

Protobuf contracts describe shared names and typed row shapes. SQLite DDL,
indexes, foreign keys, migrations, and defaults stay owned by the component
that creates the database.
