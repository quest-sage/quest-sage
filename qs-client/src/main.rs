pub mod graphics;

fn register_tracing_subscriber() {
    let subscriber = tracing_subscriber::FmtSubscriber::builder()
        .with_max_level(tracing::Level::TRACE)
        .with_env_filter("qs_common=trace,qs_client=trace")
        .finish();
    tracing::subscriber::set_global_default(subscriber).expect("Could not set tracing subscriber");
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    register_tracing_subscriber();

    let (app, event_loop) = graphics::Application::new().await;
    app.run(event_loop).await;

    Ok(())
}