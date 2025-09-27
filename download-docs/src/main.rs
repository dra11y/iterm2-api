use log::{LevelFilter, error, info, warn};
use spider::tokio;
use spider::website::Website;
use std::fs;
use std::path::Path;
use url::Url;

#[tokio::main]
async fn main() {
    // Initialize logger
    env_logger::Builder::from_default_env()
        .filter_level(LevelFilter::Debug)
        .init();

    let base_url = "https://iterm2.com/python-api/";
    let output_dir = "../docs/python-api";

    info!("🕷️  Starting iTerm2 API documentation crawler");
    info!("📥 Source URL: {base_url}");
    info!("📁 Output directory: {output_dir}");
    info!("⚙️  Configuration: 500ms delay, respecting robots.txt, no subdomains");

    // Create output directory if it doesn't exist
    if let Err(e) = fs::create_dir_all(output_dir) {
        error!("❌ Failed to create output directory: {e}");
        return;
    }
    info!("✅ Output directory created/verified");

    // Configure the website crawler
    let mut website = Website::new(base_url);
    website.with_user_agent(Some("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36"));
    website.with_respect_robots_txt(false);
    website.with_full_resources(true);
    website.with_block_assets(false);
    website.with_subdomains(false);
    website.with_whitelist_url(Some(
        [r#"^https://iterm2\.com/python-api/.*"#.into()].into(),
    ));
    website.with_blacklist_url(Some(["_downloads".into()].into()));
    // website.with_delay(500); // ms delay between requests

    // Subscribe to crawl events to process pages as they're crawled
    let mut rx = website.subscribe(16).unwrap();
    let output_dir_clone = output_dir.to_string();

    tokio::spawn(async move {
        let mut pages_processed = 0;
        let mut pages_downloaded = 0;

        while let Ok(page) = rx.recv().await {
            pages_processed += 1;
            let Ok(url) = page.get_url().parse::<Url>() else {
                warn!("Invalid URL: {:?}", page.get_url());
                continue;
            };

            // Extract path segments
            let mut path_segments: Vec<&str> =
                url.path_segments().map(|s| s.collect()).unwrap_or_default();

            if path_segments.is_empty() {
                warn!("No path segments in URL: {url}");
                continue;
            }

            // Remove the first "python-api" segment
            let dir = path_segments.remove(0);

            if dir != "python-api" {
                info!("🚫 Skipping non-API URL: {url}");
                continue;
            }

            // Build the relative file path
            let mut file_path = path_segments.join("/");

            // Handle empty path (root of python-api)
            if file_path.is_empty() {
                file_path = "index.html".to_string();
            } else if file_path.ends_with('/') {
                file_path.push_str("index.html");
            } else if !file_path.contains('.') {
                file_path.push_str(".html");
            }

            let full_path = Path::new(&output_dir_clone).join(&file_path);

            // Create parent directories if needed
            if let Some(parent) = full_path.parent() {
                if let Err(e) = fs::create_dir_all(parent) {
                    error!("❌ Failed to create directory {}: {e}", parent.display());
                    continue;
                }
            }

            // Get the HTML content and save it
            let html = page.get_html();
            if let Err(e) = fs::write(&full_path, html.as_bytes()) {
                error!("❌ Failed to write {}: {e}", full_path.display());
            } else {
                pages_downloaded += 1;
                info!("📄 [{pages_downloaded}] {url} -> {}", full_path.display());
            }
        }

        info!("✅ Crawling completed!");
        info!("📊 Summary:");
        info!("   - Pages processed: {pages_processed}");
        info!("   - Pages downloaded: {pages_downloaded}");
        info!("   - Files saved to: {output_dir_clone}/");
    });

    // Start crawling
    info!("🚀 Starting crawl...");
    website.crawl().await;
}
