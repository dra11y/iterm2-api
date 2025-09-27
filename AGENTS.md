# AGENTS.md - Technical Summary for AI

## **CRITICAL**
Use `just check` and other recipes instead of `cargo check`, etc. to enforce our linting rules.

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
- `ITerm2Connection`: Main connection factory and WebSocket communication handler
- `Window`: Object-oriented window management with tabs and sessions
- `Tab`: Object-oriented tab management with sessions and pane layout
- `Session`: Object-oriented session management with terminal operations
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
- **NEVER** add deprecation warnings or legacy methods - this is in development, APIs should be clean and intuitive from the start without backward compatibility concerns

## API Operations
- `ITerm2Connection::connect()`: Establish connection to iTerm2
- `ITerm2Connection::create_window()`: Create new window with initial tab
- `Window::create_tab()`: Create new tab in existing window
- `Tab::split_pane()`: Split tab into multiple panes
- `Session::send_text()`: Send text to specific session
- `Session::get_buffer()`: Read terminal buffer content
- `Window::activate()`: Focus specific window
- `Tab::activate()`: Focus specific tab
- `Session::activate()`: Focus specific session

## Development Workflow
- **Build**: `just generate-proto` (generates protobuf code via build.rs)
- **Proto Download**: `just download-proto` (fetches latest from iTerm2 repo)
- **Linting**: `just check` / `just fix` (uses custom dylint rules + cargo fmt)
- **README**: `just readme` (generates README.md from crate docs)

## Examples
- `basic_connection.rs`: Simple connection and tab creation
- `advanced_tabs.rs`: Multi-tab management in single window

## Build System
- `build.rs`: Compiles protobuf definitions to Rust code
- `justfile`: Task runner for common operations
- Uses protobuf 3.7.2 (4.32 is broken)

## TODO List

### High Priority (Object-Oriented API Redesign) - IN PROGRESS
- **ARCHITECTURE REDESIGN**: Replace flat `ITerm2Connection` methods with proper `Window`, `Tab`, `Session` structs that mirror Python API object hierarchy
- **Reuse protobuf objects**: Leverage existing `ListSessionsResponse::Window`, `ListSessionsResponse::Tab`, and `SessionSummary` protobuf messages as foundation for Rust structs
- **Implement object relationships**: Window contains Tabs, Tab contains Sessions, with proper weak references to prevent cycles
- **Method migration**: Move functionality from `ITerm2Connection` methods to object methods (e.g., `connection.create_tab()` → `window.create_tab()`)
- **Safety guarantees**: Implement proper validation, error handling, and thread safety with Arc<Mutex<Connection>> references

### Medium Priority (Core Object-Oriented Features)
Based on iTerm2 Python API documentation analysis, implement these key features:

#### Window Management
- **Window operations**: `activate()`, `close()`, `set_title()`, `get_frame()`, `set_frame()`
- **Window properties**: Access window number, frame, tab list
- **Window relationships**: Parent-child relationships with tabs

#### Tab Management
- **Tab operations**: `activate()`, `close()`, `set_title()`, `split_pane()`, `select_pane_in_direction()`
- **Tab properties**: Access tab ID, layout structure, session list
- **Tab relationships**: Parent-child with window and sessions

#### Session Management
- **Session operations**: `send_text()`, `activate()`, `split_pane()`, `get_buffer()`, `get_prompt()`, `run_coprocess()`, `stop_coprocess()`
- **Session properties**: Access session ID, title, frame, grid size, profile
- **Session relationships**: Parent-child with tab

#### Profile Management
- **Profile operations**: `get_property()`, `set_property()`, `list_profiles()`, `switch_profile()`
- **Profile properties**: Colors, fonts, cursor, text rendering, background

### Low Priority (Advanced Object-Oriented Features)
- **Variables system**: `get_variable()`, `set_variable()`, `monitor_variables()`, event subscriptions
- **Notification system**: `show_notification()`, custom notifications, notification handlers
- **Event subscriptions**: Session lifecycle, tab lifecycle, window lifecycle events
- **Transaction support**: Atomic operations across multiple objects
- **Saved arrangements**: Save/restore window arrangements, workspace management
- **Tool registration**: Register custom tools, tool integration

## Systematic Development Approach

### API Coverage Matrix

To guarantee comprehensive coverage, we'll create a systematic mapping of Python API features to Rust implementation:

#### Core Classes Inventory
Based on Python API documentation and protobuf analysis, we need to implement these main classes:

| Python Class | Rust Equivalent | Status | Priority |
|-------------|----------------|--------|----------|
| `App` | `ITerm2Connection` (factory) | ✅ Basic | High |
| `Window` | `Window` struct (reuses protobuf) | 🔄 Redesign | High |
| `Tab` | `Tab` struct (reuses protobuf) | 🔄 Redesign | High |
| `Session` | `Session` struct (reuses protobuf) | 🔄 Redesign | High |
| `Profile` | `Profile` management | ❌ Missing | Medium |
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

#### Phase 1: Object-Oriented API Redesign (High Priority) - IN PROGRESS
**Prerequisites**: None
**Goal**: Replace flat API with proper object hierarchy matching Python API

1.1. **Architecture Redesign** - 🔄 IN PROGRESS
- [x] Analyze protobuf objects for reuse potential
- [ ] Design Window, Tab, Session structs using protobuf foundations
- [ ] Implement proper object relationships with weak references
- [ ] Add thread-safe connection sharing with Arc<Mutex<>>
- [ ] Create conversion traits from protobuf to Rust structs

1.2. **Core Object Implementation** - 🔄 IN PROGRESS
- [ ] Implement `Window` struct with methods (create_tab, activate, close, etc.)
- [ ] Implement `Tab` struct with methods (split_pane, activate, select_pane, etc.)
- [ ] Implement `Session` struct with methods (send_text, get_buffer, split_pane, etc.)
- [ ] Update `ITerm2Connection` to be factory for objects
- [ ] Add proper error handling and validation

1.3. **API Migration** - 🔄 IN PROGRESS
- [ ] Migrate `create_window()` to return `Window` object
- [ ] Migrate `create_tab()` to `Window::create_tab()` returning `Tab` object
- [ ] Migrate `send_text()` to `Session::send_text()` method
- [ ] Migrate `get_windows()` to return `Vec<Window>` objects
- [ ] Update all examples to use new object-oriented API

#### Phase 2: Core Object Features (Medium Priority)
**Prerequisites**: Phase 1 complete
**Goal**: Implement essential iTerm2 functionality on new object architecture

2.1. **Split Pane System**
- [ ] Implement `Tab::split_pane_horizontal()` and `Tab::split_pane_vertical()`
- [ ] Implement `Session::split_pane()` for session-based splitting
- [ ] Add pane management methods (focus, resize, close)
- [ ] Implement `Tab::select_pane_in_direction()` method
- [ ] Create split_pane.rs example demonstrating object-oriented usage

2.2. **Session Operations**
- [ ] Implement `Session::get_buffer()` for terminal content access
- [ ] Implement `Session::get_prompt()` for shell prompt detection
- [ ] Implement `Session::run_coprocess()` and `Session::stop_coprocess()`
- [ ] Add session property accessors (title, frame, grid_size)
- [ ] Create session_operations.rs example

2.3. **Activation & Focus**
- [ ] Implement `Window::activate()` for window focus
- [ ] Implement `Tab::activate()` for tab focus
- [ ] Implement `Session::activate()` for session focus
- [ ] Add focus validation and error handling
- [ ] Create activation.rs example

2.4. **Profile Management**
- [ ] Implement `Session::get_profile_property()` and `Session::set_profile_property()`
- [ ] Add profile listing and switching capabilities
- [ ] Implement profile property validation
- [ ] Create profile_management.rs example

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
3. **Protobuf Analysis**: Examine protobuf definitions to understand available data structures and relationships
4. **Design**: Create Rust API that's idiomatic, safe, and aligns with Python behavior using object-oriented patterns
5. **Document**: Write comprehensive Rustdoc with examples before coding
6. **Implement**: Create proper Rust structs with methods, leveraging protobuf foundations
7. **Safety**: Implement proper error handling, validation, and thread safety
8. **Example**: Create working example demonstrating real-world object-oriented usage
9. **Test**: Write unit and integration tests ensuring correctness
10. **Review**: Verify alignment with Python API and update progress matrix

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

- **Python API Alignment**: Rust API should mirror Python capabilities where idiomatic, using object-oriented patterns
- **Idiomatic Rust**: Use Result types, proper error handling, Rust conventions, and smart pointers (Arc, Weak, Rc)
- **Object-Oriented Architecture**: Use structs with methods to represent iTerm2 objects (Window, Tab, Session) with proper relationships
- **Protobuf Reuse**: Leverage existing protobuf definitions (SessionSummary, ListSessionsResponse::Window, ListSessionsResponse::Tab) as foundation
- **Safety**: Ensure all operations are safe with proper validation, weak references to prevent cycles, and thread-safe connection sharing
- **Clarity**: Method names should clearly indicate what they do (no confusing APIs), and object hierarchy should be intuitive
- **Documentation**: Every public API must have comprehensive documentation with examples
- **Testing**: Every feature must have comprehensive test coverage including unit tests, integration tests, and example validation
- **Examples**: Every major feature should have a working, documented example demonstrating object-oriented usage
- **No Legacy Code**: Since this is in development, there is no backward compatibility, deprecation, or legacy methods - APIs are clean and intuitive from the start
