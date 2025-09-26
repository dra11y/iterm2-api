use crate::error::{Result, Error};
use std::env;

pub struct Authenticator {
    cookie: Option<String>,
    key: Option<String>,
}

impl Authenticator {
    pub fn new() -> Self {
        Self {
            cookie: env::var("ITERM2_COOKIE").ok(),
            key: env::var("ITERM2_KEY").ok(),
        }
    }
    
    pub fn has_credentials(&self) -> bool {
        self.cookie.is_some() || self.key.is_some()
    }
    
    pub fn get_auth_header(&self) -> Option<String> {
        if let Some(cookie) = &self.cookie {
            Some(format!("iTerm2-Auth-Cookie: {}", cookie))
        } else if let Some(key) = &self.key {
            Some(format!("iTerm2-Auth-Key: {}", key))
        } else {
            None
        }
    }
    
    pub async fn authenticate_via_applescript(&self) -> Result<String> {
        let output = tokio::process::Command::new("osascript")
            .arg("-e")
            .arg("tell application \"iTerm2\" to get authentication string")
            .output()
            .await?;
            
        if !output.status.success() {
            return Err(Error::Authentication(format!(
                "AppleScript failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        
        let auth_string = String::from_utf8(output.stdout)?
            .trim()
            .to_string();
            
        Ok(auth_string)
    }
}