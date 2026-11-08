set windows-shell := ["powershell.exe", "-NoLogo", "-NoProfile", "-Command"]

# Full quality gate (the same as CI)
check: fmt-check lint test coverage deny machete

# Format the code
fmt:
    cargo fmt --all

fmt-check:
    cargo fmt --all --check

lint:
    cargo clippy --workspace --all-targets --all-features -- -D warnings

test:
    cargo test --workspace --all-features

# Minimum 85% line coverage on clocky-core
coverage:
    cargo llvm-cov --package clocky-core --fail-under-lines 85

deny:
    cargo deny check

machete:
    cargo machete

# Enable the project's git hooks (run once after cloning)
setup:
    git config core.hooksPath .githooks

# Show the next version calculated from the commits (empty = no release)
next-version:
    bash scripts/next-version.sh
