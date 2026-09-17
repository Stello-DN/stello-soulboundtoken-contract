.PHONY: test build fmt clippy check interface clean

test:
	cargo test

build:
	stellar contract build

fmt:
	cargo fmt --all

clippy:
	cargo clippy --all-targets -- -D warnings

interface: build
	python3 scripts/check_sbt_interface.py

check: fmt clippy test interface

clean:
	cargo clean
