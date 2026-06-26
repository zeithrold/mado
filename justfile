set dotenv-load := false

coverage_threshold := "70"
mutation_threshold := "50"

fmt:
    cargo fmt --all

clippy:
    cargo clippy --workspace --all-targets --all-features -- -D warnings

test:
    just test-unit

test-unit:
    cargo test --workspace --lib --bins

test-integration:
    cargo test --workspace --tests

test-all: test-unit test-integration

deny:
    cargo deny check

audit:
    cargo audit

machete:
    cargo machete

udeps:
    cargo +nightly udeps --workspace --all-targets

coverage:
    cargo llvm-cov --workspace --lib --bins --fail-under-lines {{coverage_threshold}}

coverage-html:
    cargo llvm-cov --workspace --lib --bins --html

fuzz-smoke:
    cargo +nightly fuzz run icon_name -- -runs=256

fuzz:
    cargo +nightly fuzz run icon_name

mutants:
    cargo run -p xtask -- mutants target/mutants

mutants-gate:
    cargo run -p xtask -- mutants-gate {{mutation_threshold}} target/mutants

check: fmt clippy coverage

check-ci: check test-integration deny audit machete

check-full: check-ci mutants-gate udeps fuzz-smoke
