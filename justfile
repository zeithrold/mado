set dotenv-load := false

coverage_threshold := "75"
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

test-real-integration:
    MADO_RUN_REAL_INTEGRATION=1 cargo test --workspace --tests -- --ignored

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
    cargo llvm-cov --workspace --lib --bins --fail-under-lines {{ coverage_threshold }}

coverage-html:
    cargo llvm-cov --workspace --lib --bins --html

fuzz-smoke:
    cargo run -p xtask -- fuzz smoke

fuzz target="icon-name":
    cargo run -p xtask -- fuzz run {{ target }}

fuzz-nightly:
    cargo run -p xtask -- fuzz nightly

mutants:
    cargo run -p xtask -- mutants target/mutants

mutants-gate:
    cargo run -p xtask -- mutants-gate {{ mutation_threshold }} target/mutants

check: fmt clippy coverage

check-ci: check test-integration deny audit machete fuzz-smoke

check-full: check-ci mutants-gate udeps fuzz-nightly
