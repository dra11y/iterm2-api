use iterm2_api::ITerm2Connection;
use tokio;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Connecting to iTerm2...");

    let mut connection = ITerm2Connection::connect().await?;

    println!("Connected successfully!");

    // Try to list sessions
    println!("Listing sessions...");
    match connection.list_sessions().await {
        Ok(sessions) => {
            println!("Found {} sessions", sessions.len());
            for session in sessions {
                println!("  Session: {}", session.unique_identifier());
            }
        }
        Err(e) => {
            println!("Failed to list sessions: {}", e);
        }
    }

    // Try to create a new window
    println!("Creating new window...");
    match connection.create_window(None).await {
        Ok(session) => {
            println!(
                "Created window with session ID: {}",
                session.unique_identifier()
            );

            // Try to send some text
            println!("Sending 'echo Hello World' to the new session...");
            match connection
                .send_text(session.unique_identifier(), "echo Hello World\r")
                .await
            {
                Ok(()) => {
                    println!("Text sent successfully!");
                }
                Err(e) => {
                    println!("Failed to send text: {}", e);
                }
            }
        }
        Err(e) => {
            println!("Failed to create tab: {}", e);
        }
    }

    Ok(())
}
