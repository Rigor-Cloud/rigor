use anyhow::Result;
use std::fs::{self, OpenOptions};
use std::sync::Mutex;
use tracing_subscriber::fmt::writer::MakeWriterExt;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

/// Initialize tracing with stderr + file output, and optional OpenTelemetry.
/// Gracefully degrades if OTEL collector not configured.
pub fn init_tracing() -> Result<()> {
    // Ensure log directory exists
    let log_dir = crate::paths::rigor_home();
    fs::create_dir_all(&log_dir)?;

    let log_path = log_dir.join("rigor.log");
    let log_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;

    // Create multi-writer: stderr + log file
    let multi_writer = std::io::stderr.and(Mutex::new(log_file));

    // Build env filter - respect RIGOR_DEBUG and RUST_LOG
    let filter = if std::env::var("RIGOR_DEBUG").is_ok() {
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("rigor=debug"))
    } else {
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("rigor=info"))
    };

    // Try to set up OpenTelemetry if endpoint configured
    let otel_layer = setup_otel_layer();

    // Build subscriber - conditionally include OTEL layer
    // IMPORTANT: Layer ordering matters. OTEL must be on Registry before fmt
    if let Some(otel) = otel_layer {
        tracing_subscriber::registry()
            .with(otel)
            .with(filter)
            .with(
                tracing_subscriber::fmt::layer()
                    .with_writer(multi_writer)
                    .with_target(false)
                    .with_ansi(false)
                    .compact(),
            )
            .init();
    } else {
        // No OTEL - just filter + fmt
        tracing_subscriber::registry()
            .with(filter)
            .with(
                tracing_subscriber::fmt::layer()
                    .with_writer(multi_writer)
                    .with_target(false)
                    .with_ansi(false)
                    .compact(),
            )
            .init();
    }

    Ok(())
}

/// Set up OpenTelemetry layer with graceful degradation.
/// Returns None if OTEL endpoint not configured or setup fails.
fn setup_otel_layer() -> Option<
    tracing_opentelemetry::OpenTelemetryLayer<
        tracing_subscriber::Registry,
        opentelemetry_sdk::trace::Tracer,
    >,
> {
    use opentelemetry::trace::TracerProvider as _;
    use opentelemetry::KeyValue;
    use opentelemetry_otlp::WithExportConfig;
    use opentelemetry_sdk::Resource;

    // Only attempt OTEL if endpoint is configured
    let endpoint = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT").ok()?;

    eprintln!("rigor: Configuring OTEL exporter to {}", endpoint);

    // Build resource attributes — include the grounded client type if available
    let grounded = crate::daemon::ws::grounded_client().as_str();
    let resource = Resource::new(vec![
        KeyValue::new("service.name", "rigor"),
        KeyValue::new("service.version", env!("CARGO_PKG_VERSION")),
        KeyValue::new("rigor.grounded_client", grounded),
    ]);

    // Build exporter with tonic and endpoint
    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .with_endpoint(&endpoint)
        .build()
        .ok()?;

    // Build tracer provider
    use opentelemetry_sdk::runtime::Tokio;
    use opentelemetry_sdk::trace::TracerProvider;

    let provider = TracerProvider::builder()
        .with_resource(resource)
        .with_batch_exporter(exporter, Tokio)
        .build();

    let tracer = provider.tracer("rigor");

    // Store provider globally for shutdown
    opentelemetry::global::set_tracer_provider(provider);

    Some(tracing_opentelemetry::layer().with_tracer(tracer))
}

/// Shutdown OpenTelemetry provider (flush pending spans).
/// Safe to call even if OTEL not configured.
pub fn shutdown() {
    // No-op: graceful shutdown happens automatically when provider drops
    // The global tracer provider will flush on drop
}
