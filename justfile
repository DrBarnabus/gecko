set minimum-version := "1.55.1"
set default-list := true

check:
    cargo check

build:
    cargo build

build-release:
    cargo build --release

run:
    cargo run --bin gecko_app

run-trace:
    cargo run --bin gecko_app -F tracy

run-release:
    cargo run --bin gecko_app --release

format *args:
    cargo +nightly fmt {{args}}

lint:
    cargo clippy

test *args:
    cargo test {{args}}

verify:
    @just format --check
    @just lint
    @just test
