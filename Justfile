# RustyGet Rust-first task runner (replaces legacy Make workflows)

set shell := ["bash", "-euo", "pipefail", "-c"]

build_dir := "build/debug-tests"
jobs := "3"

# Default target: build Rust core and daemon
default: build

# Configure the CMake build tree (used for remaining C++ outlier translation units)
configure:
    cmake -B {{build_dir}} -DCMAKE_BUILD_TYPE=Debug -DENABLE_TESTS=ON

# Build the Rust crate directly
rust-build:
    CARGO_BUILD_JOBS={{jobs}} cargo build --manifest-path rust/Cargo.toml -j {{jobs}}

# Run Rust unit tests (pass optional filter, e.g. `just rust-test entrypoint`)
rust-test *ARGS:
    CARGO_BUILD_JOBS={{jobs}} cargo test --manifest-path rust/Cargo.toml --no-default-features -j {{jobs}} --lib {{ARGS}}

# Build the full daemon and test suite with Rust as the entry point
build:
    CARGO_BUILD_JOBS={{jobs}} cmake --build {{build_dir}} -j {{jobs}}

# Run core unit test suites
unit-test:
    cd {{build_dir}}/tests && ./nzbget_tests -t UtilTest && ./nzbget_tests -t QueueTest && ./nzbget_tests -t DupeSearchTest

# Run the PAR-first dupe fallback functional test suite against the Rust-entrypoint daemon
functional-test:
    python3 tests/functional/dupefallback/par_first_test.py --nzbget {{build_dir}}/nzbget

# Verify the migration source inventory
verify-inventory:
    python3 docs/rust-migration/verify_inventory.py

# Run all verification checks
check: (rust-test "entrypoint") build unit-test verify-inventory functional-test
