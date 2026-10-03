use robi::bootstrap::{self, AppConfig, Listen};

#[tokio::main]
async fn main() {
    robi::logs::init();

    let database_url = std::env::var("ROBI_DATABASE_URL")
        .unwrap_or_else(|_| "sqlite://robi.db?mode=rwc".to_string());
    let settings_dir = match robi::adapters::settings::home_dir() {
        Ok(dir) => dir,
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(1);
        }
    };
    let listen = match std::env::var("ROBI_BIND") {
        Ok(raw) => match bootstrap::parse_bind(&raw) {
            Ok(addr) => Listen::Exact(addr),
            Err(err) => {
                eprintln!("{err}");
                std::process::exit(1);
            }
        },
        Err(_) => Listen::Fallback {
            port: bootstrap::DEFAULT_PORT,
            attempts: bootstrap::PORT_ATTEMPTS,
        },
    };

    let state = match bootstrap::build_app_state(AppConfig {
        database_url,
        settings_dir,
    })
    .await
    {
        Ok(state) => state,
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(1);
        }
    };
    let listener = match bootstrap::bind(listen).await {
        Ok(listener) => listener,
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(1);
        }
    };
    let addr = match listener.local_addr() {
        Ok(addr) => addr,
        Err(err) => {
            eprintln!("failed to read the bound address: {err}");
            std::process::exit(1);
        }
    };
    tracing::info!(%addr, "robi-api listening");

    if let Err(err) = axum::serve(listener, bootstrap::router(state)).await {
        eprintln!("server stopped: {err}");
        std::process::exit(1);
    }
}
