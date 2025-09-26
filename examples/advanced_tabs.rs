use iterm2_api::ITerm2Connection;
use tokio;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Connecting to iTerm2...");

    let mut connection = ITerm2Connection::connect().await?;
    println!("Connected successfully!");

    // Define the working directory for our tabs
    let work_dir = "/tmp";

    // Create a new window first
    println!("Creating new window...");
    let first_session = connection.create_tab(None, None).await?;
    let first_session_id = first_session.unique_identifier().to_string();

    // Get the window ID from the window we just created
    let windows = connection.get_windows().await?;
    let window_id = windows
        .last()
        .ok_or("No windows found")?
        .window_id()
        .to_string();

    println!("Created new window with ID: {}", window_id);
    println!("Created first tab with session ID: {}", first_session_id);

    // Change to the working directory in the first tab
    let cd_command = format!("cd {}\r", work_dir);
    connection.send_text(&first_session_id, &cd_command).await?;

    let mut sessions = vec![first_session_id];

    // Create 3 more tabs in the same window
    println!("Creating 3 more tabs in the same window...");

    for i in 2..=4 {
        println!("Creating tab {}...", i);

        // Create a new tab in the same window
        let session = connection.create_tab(None, Some(&window_id)).await?;
        let session_id = session.unique_identifier().to_string();

        println!("Created tab {} with session ID: {}", i, session_id);

        // Change to the working directory
        let cd_command = format!("cd {}\r", work_dir);
        connection.send_text(&session_id, &cd_command).await?;

        sessions.push(session_id);
    }

    println!("All 4 tabs created successfully in the same window!");

    // Run 'ls' command in the first 2 tabs
    println!("Running 'ls' in first 2 tabs...");

    for (i, session_id) in sessions.iter().take(2).enumerate() {
        println!("Running ls in tab {}...", i + 1);
        connection.send_text(session_id, "ls -la\r").await?;
        println!("ls command sent to tab {}", i + 1);
    }

    println!("Advanced tab setup complete!");
    println!("Summary:");
    println!("- Created 4 tabs in {} within the same window", work_dir);
    println!("- Ran 'ls' in the first 2 tabs");
    println!("- Tabs 3 and 4 are ready for manual commands");

    Ok(())
}
