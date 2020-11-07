pub mod assets;
pub mod graphics;
pub mod ui;

fn register_tracing_subscriber() {
    let subscriber = tracing_subscriber::FmtSubscriber::builder()
        .with_max_level(tracing::Level::TRACE)
        .with_env_filter("qs_common=trace,qs_client=trace")
        .finish();
    tracing::subscriber::set_global_default(subscriber).expect("Could not set tracing subscriber");
}

/// We have to instantiate the `tokio` runtime in a bit of a roundabout way.
/// `winit` requires that all windowing code happen on the main thread, so we
/// can't just use `tokio::main`.
///
/// The solution here is to enter the tokio runtime without turning the main thread
/// into a tokio task itself. This allows us to call tokio code without allowing
/// winit's code to be sent between threads.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    register_tracing_subscriber();
    
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    
    let _guard = rt.enter();
    let (app, event_loop) = futures::executor::block_on(graphics::Application::new());
    app.run(event_loop);

    Ok(())
}
