# AGENTS.md - Technical Summary for AI

## Project Overview
Rust library for programmatically controlling iTerm2 via its official WebSocket API over Unix domain socket. Alternative to Python API.

**API Documentation**: [docs/...](docs/...) (downloaded from https://iterm2.com/python-api)

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

## API Server Clarification
This library uses iTerm2's "Python API" server setting, but this is misleading naming. The actual communication is via Protocol Buffers over WebSocket - Python is NOT an intermediary and this library uses no Python bindings. The "Python API" setting simply enables iTerm2's WebSocket API server.

## Forbidden Patterns
- **NEVER** use ITERM2_COOKIE environment variable - this only works for Python scripts launched from iTerm2's Scripts menu (Rust apps CANNOT be added to this menu)
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

## TODO List

### High Priority (API Foundation)
- **Fix API clarity**: `create_tab()` method is confusing - `create_tab(None, None)` creates a new window, not a tab. Consider making window_id required or renaming method to be clearer about window vs tab creation
- **DOCUMENT ALL** public methods, functions, structs, and enums+variants with comprehensive Rustdoc comments
- **IMPROVE example docs**: Add more comprehensive examples with better documentation

### Medium Priority (Core API Features)
Based on iTerm2 Python API documentation analysis, implement these key features:

#### Window and Tab Management
- **Split pane functionality**: Implement `split_pane` methods for horizontal/vertical splits
- **Session activation**: Add methods to focus/activate specific sessions and tabs
- **Window arrangement**: Implement saved arrangements and window positioning

#### Profile Management
- **Get profile properties**: Add methods to retrieve profile settings (colors, fonts, etc.)
- **Set profile properties**: Add methods to modify profile settings dynamically
- **Profile switching**: Implement methods to change session profiles

#### Variables System
- **Variable monitoring**: Implement the variables system for monitoring iTerm2 state changes
- **Variable setting**: Add methods to set custom variables
- **Event subscriptions**: Add support for subscribing to variable change events

#### Advanced Features
- **Buffer access**: Implement `get_buffer` methods for reading terminal content
- **Prompt detection**: Add `get_prompt` functionality for detecting shell prompts
- **Notification system**: Implement iTerm2 notification capabilities
- **Tool registration**: Add support for registering custom tools

### Low Priority (Enhanced Functionality)
- **Transaction support**: Implement transaction-based operations for atomic changes
- **Property system**: Add generic get/set property methods
- **Injection system**: Implement custom escape sequence injection
- **Saved arrangements**: Add methods for saving and restoring window arrangements

## Systematic Development Approach

### API Coverage Matrix

To guarantee comprehensive coverage, we'll create a systematic mapping of Python API features to Rust implementation:

#### Core Classes Inventory
Based on Python API documentation, we need to implement these main classes:

| Python Class | Rust Equivalent | Status | Priority |
|-------------|----------------|--------|----------|
| `App` | `ITerm2Connection` methods | ✅ Basic | High |
| `Session` | `SessionSummary` + methods | ✅ Basic | High |
| `Tab` | Tab management methods | ✅ Basic | High |
| `Window` | Window management methods | ✅ Basic | High |
| `Profile` | Profile management | ❌ Missing | Medium |
| `Color` | Color utilities | ❌ Missing | Medium |
| `Variable` | Variables system | ❌ Missing | Medium |
| `Screen` | Buffer access | ❌ Missing | Medium |
| `Tool` | Tool registration | ❌ Missing | Low |

#### Method Mapping System
For each Python API method, we'll track:

1. **Method signature**: Python → Rust translation
2. **Dependencies**: What other methods/features it depends on
3. **Implementation status**: Not started → In progress → Complete → Tested
4. **Documentation status**: Missing → Draft → Complete
5. **Example status**: Missing → Basic → Comprehensive

### Implementation Phases with Dependencies

#### Phase 1: Foundation & Core API (High Priority)
**Prerequisites**: None
**Goal**: Basic functionality working with clear API

1.1. **API Clarity Refactor**
- [ ] Separate `create_window()` from `create_tab()`
- [ ] Update all examples and documentation
- [ ] Test backward compatibility

1.2. **Comprehensive Documentation**
- [ ] Document all existing public methods
- [ ] Add Rustdoc examples for each method
- [ ] Document error types and handling

1.3. **Enhanced Examples**
- [ ] Improve basic_connection.rs with error handling
- [ ] Add comprehensive advanced_tabs.rs
- [ ] Create error_handling.rs example

#### Phase 2: Essential Features (Medium Priority)
**Prerequisites**: Phase 1 complete
**Goal**: Core iTerm2 functionality for terminal automation

2.1. **Split Pane System**
- [ ] Research Python `split_pane` methods
- [ ] Implement `split_pane_horizontal()`
- [ ] Implement `split_pane_vertical()`
- [ ] Add pane management methods
- [ ] Create split_pane.rs example

2.2. **Profile Management**
- [ ] Research Python profile API
- [ ] Implement `get_profile_property()`
- [ ] Implement `set_profile_property()`
- [ ] Add profile listing methods
- [ ] Create profile_management.rs example

2.3. **Session Activation & Focus**
- [ ] Research Python activation methods
- [ ] Implement `activate_session()`
- [ ] Implement `activate_tab()`
- [ ] Implement `activate_window()`
- [ ] Create session_activation.rs example

2.4. **Variables System**
- [ ] Research Python variables API
- [ ] Implement `get_variable()`
- [ ] Implement `set_variable()`
- [ ] Implement variable monitoring
- [ ] Create variables.rs example

#### Phase 3: Advanced Features (Medium Priority)
**Prerequisites**: Phase 2 complete
**Goal**: Advanced automation and monitoring capabilities

3.1. **Buffer & Screen Access**
- [ ] Research Python buffer API
- [ ] Implement `get_buffer()`
- [ ] Implement `get_prompt()`
- [ ] Add screen content analysis
- [ ] Create buffer_access.rs example

3.2. **Notification System**
- [ ] Research Python notification API
- [ ] Implement `show_notification()`
- [ ] Add notification customization
- [ ] Create notifications.rs example

3.3. **Event Subscriptions**
- [ ] Research Python event system
- [ ] Implement event subscription framework
- [ ] Add common event handlers
- [ ] Create events.rs example

#### Phase 4: Enhanced Functionality (Low Priority)
**Prerequisites**: Phase 3 complete
**Goal**: Professional-grade features for complex workflows

4.1. **Transaction Support**
- [ ] Research Python transaction API
- [ ] Implement transaction framework
- [ ] Add atomic operations
- [ ] Create transactions.rs example

4.2. **Generic Property System**
- [ ] Research Python property API
- [ ] Implement generic `get_property()`
- [ ] Implement generic `set_property()`
- [ ] Create properties.rs example

4.3. **Saved Arrangements**
- [ ] Research Python arrangement API
- [ ] Implement save/restore arrangements
- [ ] Add arrangement management
- [ ] create arrangements.rs example

### Quality Assurance System

#### Documentation-Driven Development
For each feature:
1. **Read Python API docs** → Extract specification
2. **Design Rust API** → Create idiomatic interface
3. **Write Rustdoc** → Document before implementation
4. **Write example** → Create usage demonstration
5. **Implement** → Write the actual code
6. **Test** → Verify against Python behavior

#### Testing Strategy
- **Unit tests**: For each individual method
- **Integration tests**: For multi-step workflows
- **Example tests**: Verify all examples work
- **Documentation tests**: Ensure code examples compile
- **Error handling tests**: Verify proper error cases

#### Progress Tracking
Create a `PROGRESS.md` file with:
- Feature completion status
- Documentation coverage percentage
- Test coverage percentage
- Python API alignment percentage
- Next steps and blockers

### Implementation Strategy

For each feature:
1. **Research (RST Files)**: Extract complete method signatures and API surface from source RST files
2. **Research (HTML Files)**: Understand detailed implementation requirements and usage patterns from HTML docs
3. **Design**: Create Rust API that's idiomatic, safe, and aligns with Python behavior
4. **Document**: Write comprehensive Rustdoc with examples before coding
5. **Implement**: Add methods to `ITerm2Connection` struct with proper error handling
6. **Example**: Create working example demonstrating real-world usage
7. **Test**: Write unit and integration tests ensuring correctness
8. **Review**: Verify alignment with Python API and update progress matrix

### Documentation Source Strategy

**RST Files (Source Documentation) - For API Discovery**
- **Purpose**: Extract complete method inventories and class structures
- **Method**: Parse `:members:` directives to get all available methods
- **Files**: `iTerm2/api/library/python/iterm2/docs/*.rst` files (app.rst, session.rst, tab.rst, etc.)
- **Output**: Complete API coverage matrix

**HTML Files (Built Documentation) - For Implementation Details**
- **Purpose**: Understand method parameters, return values, and usage patterns
- **Method**: Read detailed descriptions and examples
- **Files**: `docs/python-api/` directory
- **Output**: Implementation specifications and best practices

### Key Design Principles

- **Python API Alignment**: Rust API should mirror Python capabilities where idiomatic
- **Idiomatic Rust**: Use Result types, proper error handling, and Rust conventions
- **Safety**: Ensure all operations are safe and handle edge cases gracefully
- **Clarity**: Method names should clearly indicate what they do
- **Documentation**: Every public API must have comprehensive documentation with examples
- **Testing**: Every feature must have comprehensive test coverage
- **Examples**: Every major feature should have a working, documented example
