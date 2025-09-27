//! A Rust library for programmatically controlling iTerm2 via its official API over Unix domain socket without Python.
//!
//! ## Prerequisites
//!
//! Before using this library, you must enable iTerm2's API server and grant permission:
//!
//! 1. Open iTerm2
//! 2. Go to **Settings > General > Magic**
//! 3. Check **"Enable Python API"**
//! 4. Optional: To avoid a permission prompt every time you run your helper app, change **"Require 'Automation' permission"** to **"Allow all apps to connect"**
//!
//! ## Basic Usage
//!
//! ```rust,no_run
//! use iterm2_api::ITerm2Connection;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Connect to iTerm2
//!     let mut connection = ITerm2Connection::connect().await?;
//!
//!     // Create a new window with a single tab
//!     let session = connection.create_window(None).await?;
//!
//!     // Send a command to the new session
//!     connection.send_text(session.unique_identifier(), "echo Hello World\r").await?;
//!
//!     Ok(())
//! }
//! ```
//!
//! ## Examples
//!
//! ### Advanced Tab Management
//!
//! ```rust,no_run
//! use iterm2_api::ITerm2Connection;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let mut connection = ITerm2Connection::connect().await?;
//!
//!     // Create first window to establish a window_id
//!     let first_session = connection.create_window(None).await?;
//!     let windows = connection.get_windows().await?;
//!     let window_id = windows.first().unwrap().window_id().to_string();
//!
//!     // Create 3 more tabs in the same window
//!     let work_dir = "/tmp";
//!     let mut sessions = vec![first_session.unique_identifier().to_string()];
//!
//!     for i in 2..=4 {
//!         let session = connection.create_tab(None, &window_id).await?;
//!         let session_id = session.unique_identifier().to_string();
//!
//!         // Change to working directory
//!         let cd_command = format!("cd {}\r", work_dir);
//!         connection.send_text(&session_id, &cd_command).await?;
//!
//!         sessions.push(session_id);
//!     }
//!
//!     // Run commands in specific tabs
//!     for session_id in sessions.iter().take(2) {
//!         connection.send_text(session_id, "ls -la\r").await?;
//!     }
//!
//!     Ok(())
//! }
//! ```
//!
//! See the `examples/` directory for more comprehensive usage examples.

pub mod connection;
pub mod error;
pub mod generated;

pub use connection::ITerm2Connection;
pub use error::{Error, Result};
