# Download iTerm2 API proto file from GitHub
download-proto:
    #!/usr/bin/env bash
    set -e
    mkdir -p proto
    curl -o proto/api.proto https://raw.githubusercontent.com/gnachman/iTerm2/master/proto/api.proto
    echo "Downloaded api.proto to proto/"

# Generate Rust code from proto files
generate-proto:
    #!/usr/bin/env bash
    set -e
    cargo build --build-only