#[tokio::main]
async fn main() {
    if let Err(error) = reinforce_server::run().await {
        eprintln!("reinforce: {} ({})", error.message, error.code);
        std::process::exit(1);
    }
}
