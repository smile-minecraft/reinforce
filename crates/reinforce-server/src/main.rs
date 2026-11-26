#[tokio::main]
async fn main() {
    if let Err(error) = reinforce_server::run().await {
        eprintln!(
            "reinforce: {} ({})",
            error.diagnostic.as_deref().unwrap_or(&error.message),
            error.code
        );
        std::process::exit(1);
    }
}
