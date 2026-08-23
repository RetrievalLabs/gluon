SHELL := /usr/bin/env bash

ROOT_DIR := $(abspath $(dir $(lastword $(MAKEFILE_LIST))))
CODE_PARSER_DIR := $(ROOT_DIR)/app/code-parser
HARNESS_DIR := $(ROOT_DIR)/app/harness
PROTO_DIR := $(ROOT_DIR)/app/package
PROTO_FILES := $(shell find "$(PROTO_DIR)" -name '*.proto' | sort)
BIN_DIR := $(ROOT_DIR)/bin
GLUON_CLI := $(BIN_DIR)/gluon-cli
CODE_PARSER_BIN := $(CODE_PARSER_DIR)/target/release/code-parser

.PHONY: all build gluon-cli proto proto-clients proto-python proto-rust proto-python-client proto-rust-client test clean

all: gluon-cli

build: gluon-cli

gluon-cli:
	cargo build --release --manifest-path "$(CODE_PARSER_DIR)/Cargo.toml"
	mkdir -p "$(BIN_DIR)"
	cp "$(CODE_PARSER_BIN)" "$(GLUON_CLI)"
	chmod +x "$(GLUON_CLI)"
	@echo "wrote $(GLUON_CLI)"

proto: proto-clients

proto-clients: proto-python-client proto-rust-client

proto-python: proto-python-client

proto-rust: proto-rust-client

proto-python-client:
	mkdir -p "$(HARNESS_DIR)/generated"
	cd "$(HARNESS_DIR)" && uv run python -m grpc_tools.protoc -I "$(PROTO_DIR)" --python_out "$(HARNESS_DIR)/generated" $(PROTO_FILES)
	touch "$(HARNESS_DIR)/generated/__init__.py"
	touch "$(HARNESS_DIR)/generated/gluon/__init__.py"
	touch "$(HARNESS_DIR)/generated/gluon/db/__init__.py"
	touch "$(HARNESS_DIR)/generated/gluon/db/v1/__init__.py"
	@echo "generated Python proto clients in $(HARNESS_DIR)/generated"

proto-rust-client:
	cargo check --manifest-path "$(CODE_PARSER_DIR)/Cargo.toml"
	@echo "generated Rust proto client through $(CODE_PARSER_DIR)/build.rs"

test:
	cargo test --manifest-path "$(CODE_PARSER_DIR)/Cargo.toml"

clean:
	cargo clean --manifest-path "$(CODE_PARSER_DIR)/Cargo.toml"
	rm -f "$(GLUON_CLI)"
