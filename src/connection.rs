use crate::error::{Error, Result};
use crate::generated::api::*;
use futures_util::{SinkExt, StreamExt};
use protobuf::Message as ProtobufMessage;
use tokio::net::UnixStream;
use tokio_tungstenite::tungstenite::{Message, http::Request};
use tokio_tungstenite::{WebSocketStream, client_async};

pub struct ITerm2Connection {
    websocket: WebSocketStream<UnixStream>,
}

impl ITerm2Connection {
    /// Connect to iTerm2 via Unix domain socket.
    /// 
    /// This establishes a WebSocket connection to iTerm2's API server. iTerm2 must be
    /// running with the API server enabled in Settings > General > Magic.
    /// 
    /// # Returns
    /// A connected `ITerm2Connection` instance
    /// 
    /// # Errors
    /// Returns `Error::Connection` if:
    /// - iTerm2 is not running
    /// - The API server is not enabled
    /// - The Unix domain socket cannot be found or accessed
    /// - The WebSocket handshake fails
    /// 
    /// # Example
    /// ```rust,no_run
    /// # use iterm2_api::ITerm2Connection;
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let mut connection = ITerm2Connection::connect().await?;
    /// println!("Connected to iTerm2 successfully!");
    /// # Ok(())
    /// # }
    /// ```
    pub async fn connect() -> Result<Self> {
        // Unix domain socket is the ONLY way to connect to iTerm2
        let socket_path = dirs::home_dir()
            .unwrap_or_default()
            .join("Library/Application Support/iTerm2/private/socket");

        if !socket_path.exists() {
            return Err(Error::Connection(format!(
                "iTerm2 Unix domain socket not found at: {}. iTerm2 must be running with API server enabled.",
                socket_path.display()
            )));
        }

        let stream = UnixStream::connect(&socket_path).await.map_err(|e| {
            Error::Connection(format!("Failed to connect to Unix domain socket: {e}"))
        })?;

        // Create a WebSocket request for the Unix domain socket with required headers
        let request = Request::builder()
            .uri("ws://localhost/")
            .header("Host", "localhost")
            .header("Upgrade", "websocket")
            .header("Connection", "Upgrade")
            .header("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ==")
            .header("Sec-WebSocket-Version", "13")
            .header("Sec-WebSocket-Protocol", "api.iterm2.com")
            .header("Origin", "ws://localhost/")
            // Required header
            .header("x-iterm2-library-version", "rust 1.0")
            .body(())
            .map_err(|e| Error::Connection(format!("Failed to build WebSocket request: {e}")))?;

        // Perform the WebSocket handshake using client_async
        let (websocket, response) = client_async(request, stream)
            .await
            .map_err(|e| Error::Connection(format!("WebSocket handshake failed: {e}")))?;

        // Check if we got a successful response
        if response.status() != 101 {
            return Err(Error::Connection(format!(
                "WebSocket handshake failed with status {}: {}. Make sure iTerm2 has 'Allow all apps to connect' enabled in Settings > General > Magic, or run this script from iTerm2.",
                response.status(),
                response
                    .status()
                    .canonical_reason()
                    .unwrap_or("Unknown reason")
            )));
        }

        Ok(Self { websocket })
    }

    pub async fn send_message(&mut self, message: ClientOriginatedMessage) -> Result<()> {
        let mut bytes = Vec::new();
        message.write_to_vec(&mut bytes)?;

        self.websocket.send(Message::Binary(bytes.into())).await?;
        Ok(())
    }

    pub async fn receive_message(&mut self) -> Result<ServerOriginatedMessage> {
        match self.websocket.next().await {
            Some(Ok(Message::Binary(data))) => {
                let message = ServerOriginatedMessage::parse_from_bytes(&data)?;
                Ok(message)
            }
            Some(Ok(msg)) => Err(Error::Connection(format!(
                "Unexpected message type: {msg:?}"
            ))),
            Some(Err(e)) => Err(Error::WebSocket(e)),
            None => Err(Error::Connection("Connection closed".to_string())),
        }
    }

    /// Create a new window with a single tab.
    /// 
    /// This is equivalent to creating a new iTerm2 window. If you want to create
    /// a tab within an existing window, use `create_tab()` instead.
    /// 
    /// # Arguments
    /// * `profile_name` - Optional profile name to use for the new tab
    /// 
    /// # Returns
    /// A `SessionSummary` for the newly created session in the new window
    /// 
    /// # Example
    /// ```rust,no_run
    /// # use iterm2_api::ITerm2Connection;
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let mut connection = ITerm2Connection::connect().await?;
    /// let session = connection.create_window(None).await?;
    /// println!("Created new window with session: {}", session.unique_identifier());
    /// # Ok(())
    /// # }
    /// ```
    pub async fn create_window(&mut self, profile_name: Option<&str>) -> Result<SessionSummary> {
        let mut request = CreateTabRequest::new();
        if let Some(profile) = profile_name {
            request.set_profile_name(profile.to_string());
        }
        // No window_id means create a new window

        let mut message = ClientOriginatedMessage::new();
        message.set_create_tab_request(request);

        self.send_message(message).await?;

        let response = self.receive_message().await?;

        if response.has_create_tab_response() {
            let create_response = response.create_tab_response();
            if create_response.status() == create_tab_response::Status::OK {
                let mut session = SessionSummary::new();
                session.set_unique_identifier(create_response.session_id().to_string());
                Ok(session)
            } else {
                Err(Error::Api(format!(
                    "Create window failed: {:?}",
                    create_response.status()
                )))
            }
        } else {
            Err(Error::Api("Expected create tab response".to_string()))
        }
    }

    /// Create a new tab in an existing window.
    /// 
    /// This creates a new tab within the specified window. If you want to create
    /// a new window, use `create_window()` instead.
    /// 
    /// # Arguments
    /// * `profile_name` - Optional profile name to use for the new tab
    /// * `window_id` - The ID of the window to create the tab in
    /// 
    /// # Returns
    /// A `SessionSummary` for the newly created session
    /// 
    /// # Example
    /// ```rust,no_run
    /// # use iterm2_api::ITerm2Connection;
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let mut connection = ITerm2Connection::connect().await?;
    /// 
    /// // First create a window to get a window_id
    /// let first_session = connection.create_window(None).await?;
    /// let windows = connection.get_windows().await?;
    /// let window_id = windows.first().unwrap().window_id().to_string();
    /// 
    /// // Now create a tab in that window
    /// let session = connection.create_tab(None, &window_id).await?;
    /// println!("Created new tab with session: {}", session.unique_identifier());
    /// # Ok(())
    /// # }
    /// ```
    pub async fn create_tab(
        &mut self,
        profile_name: Option<&str>,
        window_id: &str,
    ) -> Result<SessionSummary> {
        let mut request = CreateTabRequest::new();
        if let Some(profile) = profile_name {
            request.set_profile_name(profile.to_string());
        }
        request.set_window_id(window_id.to_string());

        let mut message = ClientOriginatedMessage::new();
        message.set_create_tab_request(request);

        self.send_message(message).await?;

        let response = self.receive_message().await?;

        if response.has_create_tab_response() {
            let create_response = response.create_tab_response();
            if create_response.status() == create_tab_response::Status::OK {
                let mut session = SessionSummary::new();
                session.set_unique_identifier(create_response.session_id().to_string());
                Ok(session)
            } else {
                Err(Error::Api(format!(
                    "Create tab failed: {:?}",
                    create_response.status()
                )))
            }
        } else {
            Err(Error::Api("Expected create tab response".to_string()))
        }
    }

    

    /// Send text to a specific session.
    /// 
    /// This sends the specified text to the terminal session identified by `session_id`.
    /// The text is sent as if typed by the user. Include carriage returns (`\r`) to execute commands.
    /// 
    /// # Arguments
    /// * `session_id` - The unique identifier of the session to send text to
    /// * `text` - The text to send to the session
    /// 
    /// # Returns
    /// `Ok(())` if the text was sent successfully
    /// 
    /// # Errors
    /// Returns `Error::Api` if the session doesn't exist or the send operation fails
    /// 
    /// # Example
    /// ```rust,no_run
    /// # use iterm2_api::ITerm2Connection;
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let mut connection = ITerm2Connection::connect().await?;
    /// let session = connection.create_window(None).await?;
    /// 
    /// // Send a command (note the \r to execute)
    /// connection.send_text(session.unique_identifier(), "echo Hello World\r").await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn send_text(&mut self, session_id: &str, text: &str) -> Result<()> {
        let mut request = SendTextRequest::new();
        request.set_session(session_id.to_string());
        request.set_text(text.to_string());

        let mut message = ClientOriginatedMessage::new();
        message.set_send_text_request(request);

        self.send_message(message).await?;

        let response = self.receive_message().await?;

        if response.has_send_text_response() {
            let send_response = response.send_text_response();
            if send_response.status() == send_text_response::Status::OK {
                Ok(())
            } else {
                Err(Error::Api(format!(
                    "Send text failed: {:?}",
                    send_response.status()
                )))
            }
        } else {
            Err(Error::Api("Expected send text response".to_string()))
        }
    }

    /// List all available sessions.
    /// 
    /// This returns a list of all sessions that are currently available. Note that
    /// this primarily returns buried sessions (sessions that are not in any window).
    /// For sessions in windows, use `get_windows()` and examine the window structure.
    /// 
    /// # Returns
    /// A vector of `SessionSummary` objects representing available sessions
    /// 
    /// # Errors
    /// Returns `Error::Api` if the list operation fails
    /// 
    /// # Example
    /// ```rust,no_run
    /// # use iterm2_api::ITerm2Connection;
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let mut connection = ITerm2Connection::connect().await?;
    /// let sessions = connection.list_sessions().await?;
    /// 
    /// println!("Found {} sessions:", sessions.len());
    /// for session in sessions {
    ///     println!("  Session: {}", session.unique_identifier());
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn list_sessions(&mut self) -> Result<Vec<SessionSummary>> {
        let request = ListSessionsRequest::new();

        let mut message = ClientOriginatedMessage::new();
        message.set_list_sessions_request(request);

        self.send_message(message).await?;

        let response = self.receive_message().await?;

        if response.has_list_sessions_response() {
            let list_response = response.list_sessions_response();
            // ListSessionsResponse doesn't have a status field, it just contains windows and buried_sessions
            let mut sessions = Vec::new();
            for _window in &list_response.windows {
                // Extract sessions from windows
                // This is a simplified approach - we'd need to examine the Window structure
                // For now, let's return buried sessions which are already SessionSummary objects
            }
            sessions.extend(list_response.buried_sessions.clone());
            Ok(sessions)
        } else {
            Err(Error::Api("Expected list sessions response".to_string()))
        }
    }

    /// Get all iTerm2 windows.
    /// 
    /// This returns a list of all currently open iTerm2 windows, including their
    /// tabs and sessions. Each window contains information about its tabs and sessions.
    /// 
    /// # Returns
    /// A vector of `Window` objects representing all open iTerm2 windows
    /// 
    /// # Errors
    /// Returns `Error::Api` if the get windows operation fails
    /// 
    /// # Example
    /// ```rust,no_run
    /// # use iterm2_api::ITerm2Connection;
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let mut connection = ITerm2Connection::connect().await?;
    /// let windows = connection.get_windows().await?;
    /// 
    /// println!("Found {} windows:", windows.len());
    /// for window in &windows {
    ///     println!("  Window ID: {}", window.window_id());
    ///     println!("  Number of tabs: {}", window.tabs.len());
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn get_windows(&mut self) -> Result<Vec<list_sessions_response::Window>> {
        let request = ListSessionsRequest::new();

        let mut message = ClientOriginatedMessage::new();
        message.set_list_sessions_request(request);

        self.send_message(message).await?;

        let response = self.receive_message().await?;

        if response.has_list_sessions_response() {
            let list_response = response.list_sessions_response();
            Ok(list_response.windows.clone())
        } else {
            Err(Error::Api("Expected list sessions response".to_string()))
        }
    }
}
