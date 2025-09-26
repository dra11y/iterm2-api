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

    pub async fn create_tab(
        &mut self,
        profile_name: Option<&str>,
        window_id: Option<&str>,
    ) -> Result<SessionSummary> {
        let mut request = CreateTabRequest::new();
        if let Some(profile) = profile_name {
            request.set_profile_name(profile.to_string());
        }
        if let Some(window) = window_id {
            request.set_window_id(window.to_string());
        }

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
