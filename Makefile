SHELL := /usr/bin/env bash

ROOT_DIR := $(abspath $(dir $(lastword $(MAKEFILE_LIST))))
CODE_PARSER_DIR := $(ROOT_DIR)/app/code-parser
BIN_DIR := $(ROOT_DIR)/bin
GLUON_CLI := $(BIN_DIR)/gluon-cli
CODE_PARSER_BIN := $(CODE_PARSER_DIR)/target/release/code-parser

.PHONY: all build gluon-cli test clean

all: gluon-cli

build: gluon-cli

gluon-cli:
	cargo build --release --manifest-path "$(CODE_PARSER_DIR)/Cargo.toml"
	mkdir -p "$(BIN_DIR)"
	cp "$(CODE_PARSER_BIN)" "$(GLUON_CLI)"
	chmod +x "$(GLUON_CLI)"
	@echo "wrote $(GLUON_CLI)"

test:
	cargo test --manifest-path "$(CODE_PARSER_DIR)/Cargo.toml"

clean:
	cargo clean --manifest-path "$(CODE_PARSER_DIR)/Cargo.toml"
	rm -f "$(GLUON_CLI)"
