set dotenv-load := false

coverage_threshold := "70"
mutation_threshold := "50"

fmt:
    cargo fmt --all

clippy:
    cargo clippy --workspace --all-targets --all-features -- -D warnings

test:
    cargo test --workspace --all-targets

test-unit:
    cargo test --workspace --lib --bins

test-integration:
    cargo test --workspace --tests

deny:
    cargo deny check

audit:
    cargo audit

machete:
    cargo machete

udeps:
    cargo +nightly udeps --workspace --all-targets

coverage:
    cargo llvm-cov --workspace --all-targets --fail-under-lines {{coverage_threshold}}

coverage-html:
    cargo llvm-cov --workspace --all-targets --html

fuzz-smoke:
    cargo +nightly fuzz run icon_name -- -runs=256

fuzz:
    cargo +nightly fuzz run icon_name

mutants:
    cargo mutants --workspace --copy-target true --output target/mutants

mutants-gate:
    ./scripts/mutants-gate.sh {{mutation_threshold}} target/mutants

check: fmt clippy test deny audit machete coverage mutants-gate

check-full: check udeps fuzz-smoke
