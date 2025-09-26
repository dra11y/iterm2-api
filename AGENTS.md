# AGENTS.md - Technical Summary for AI

## Project Overview
Rust library for programmatically controlling iTerm2 via its official WebSocket API over Unix domain socket. Alternative to Python API.

## Core Architecture
- **Protocol**: WebSocket over Unix domain socket at `~/Library/Application Support/iTerm2/private/socket`
- **Serialization**: Protocol Buffers (proto2) from `proto/api.proto` (downloaded from iTerm2 upstream)
- **Async Runtime**: Tokio with full feature set
- **WebSocket**: tokio-tungstenite with native-tls support
- **Security**: iTerm2 handles authentication via user prompts or "Allow all apps to connect" mode

## Key Components
- `ITerm2Connection`: Main connection struct handling WebSocket communication
- `Error`: Custom error types for WebSocket, IO, Protobuf, Authentication, Connection, API
- `generated/`: Rust code generated from protobuf definitions

## Authentication Methods
1. **User Prompt**: iTerm2 prompts user for permission when connecting (default)
2. **No Auth**: When "Allow all apps to connect" is enabled in iTerm2 settings

## Important Note
Rust apps cannot be launched from iTerm2's Scripts menu (Python-only). Cookie authentication (ITERM2_COOKIE) is impossible for Rust apps since iTerm2 only launches Python scripts from the Scripts menu.

## Forbidden Patterns
- **NEVER** use ITERM2_COOKIE environment variable - this only works for Python scripts launched from iTerm2's Scripts menu
- **NEVER** reference AppleScript - this is a pure Rust project with no AppleScript integration
- **NEVER** attempt cookie-based authentication - iTerm2 handles authentication automatically via its security model

## API Operations
- `create_tab()`: Create new tab (optionally in specific window)
- `send_text()`: Send text to specific session
- `list_sessions()`: List all sessions
- `get_windows()`: Get window information

## Development Workflow
- **Build**: `cargo build` (generates protobuf code via build.rs)
- **Proto Download**: `just download-proto` (fetches latest from iTerm2 repo)
- **Proto Generation**: `just generate-proto` (runs cargo build --build-only)
- **Linting**: `just fix` (uses custom dylint rules + cargo fmt)
- **README**: `just readme` (generates README.md from crate docs)

## Dependencies
- **Core**: tokio, tokio-tungstenite, futures-util, protobuf
- **Serialization**: serde, serde_json, base64, uuid
- **Error Handling**: thiserror
- **Utilities**: dirs, url, tracing

## Examples
- `basic_connection.rs`: Simple connection and tab creation
- `advanced_tabs.rs`: Multi-tab management in single window

## Build System
- `build.rs`: Compiles protobuf definitions to Rust code
- `justfile`: Task runner for common operations
- Uses protobuf 3.7.2 (4.32 is broken)