# Run cargo check
check:
    cargo dylint --all

# Lint and fix with dylint / Tom's custom lints
fix:
    cargo dylint --all --fix -- --allow-dirty
    cargo fmt

# Run the api-parser to analyze iTerm2 python API code
api *args="":
    @RUST_LOG=error cargo run --quiet -p api-parser -- -s iTerm2/api/library/python/iterm2/ {{args}}

[working-directory: 'download-docs']
download-docs:
    cargo run

# Download iTerm2 API docs using our custom Rust crawler
download-api-docs-rust:
    cd download-docs && RUST_LOG=info cargo run

# Install global crate if not found (installs binstall first if not installed)
@binstall what="" which=what:
    (which {{which}} 2>&1 >/dev/null) || (cargo binstall --force -y {{what}}) || (cargo install --locked cargo-binstall && cargo binstall --force -y {{what}})

readme: (binstall 'cargo-readme')
    cargo readme > README.md

# Download iTerm2 API proto file from GitHub
download-proto:
    #!/usr/bin/env bash
    set -e
    mkdir -p proto
    curl -o proto/api.proto https://raw.githubusercontent.com/gnachman/iTerm2/master/proto/api.proto
    echo "Downloaded api.proto to proto/"

# Generate Rust code from .proto file
generate-proto:
    cargo build --build-only
