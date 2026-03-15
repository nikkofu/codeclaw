SHELL := /bin/bash

VERSION := $(shell awk -F'"' '/^version = "/ { print $$2; exit }' Cargo.toml)
HANDOVER_PACK_DIR ?= tmp/handover-pack/manual-$(VERSION)

.PHONY: help check test release-check release-check-fast handover-pack

help:
	@echo "CodeClaw developer targets"
	@echo
	@echo "  make check                Run cargo check"
	@echo "  make test                 Run cargo test --quiet"
	@echo "  make release-check        Run full release metadata and regression checks"
	@echo "  make release-check-fast   Run release metadata checks without cargo check/test"
	@echo "  make handover-pack        Generate handover pack into $(HANDOVER_PACK_DIR)"

check:
	cargo check

test:
	cargo test --quiet

release-check:
	bash scripts/release_check.sh

release-check-fast:
	bash scripts/release_check.sh --skip-check --skip-test

handover-pack:
	bash scripts/handover_pack.sh $(HANDOVER_PACK_DIR)
