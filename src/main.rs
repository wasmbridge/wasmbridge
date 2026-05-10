mod admin;
mod hardware_id;
mod modules;

use anyhow::Context;
use wasmbridge_client_proto::control_plane::{
    ClientEvent, CommandResponse, cloud_command::Command,
};
use wasmbridge_client_proto::prelude::*;
use wintray::WintrayAppBuilder;
use wintray::config::load_config;

#[cfg(not(windows))]
compile_error!("WasmBridge currently only supports Windows.");

#[cfg(windows)]
fn main() -> anyhow::Result<()> {
    // Initialize the shared plugin registry
    let registry = modules::ModuleRegistry::new();

    // Determine the directory for plugins (AppData/WasmBridge/plugins)
    let mut plugins_dir = std::env::var("APPDATA")
        .map(std::path::PathBuf::from)
        .context("AppData directory not found")?;
    plugins_dir.push("WasmBridge");
    plugins_dir.push("plugins");

    // Load existing WASM plugins from the disk
    if let Err(e) = registry.load_all_from_dir(plugins_dir) {
        eprintln!("Error loading initial plugins: {}", e);
    }

    // Load application configuration (server URL, port, tokens, etc.)
    let config: admin::AppConfig = load_config();
    let address = format!("127.0.0.1:{}", config.port);

    // Initialize the shared Tokio runtime
    let rt = tokio::runtime::Runtime::new().context("Failed to initialize Tokio runtime")?;
    let _guard = rt.enter();

    // Explicitly install the cryptography provider for rustls (required in 0.23+)
    rustls::crypto::ring::default_provider().install_default().ok();

    // ==========================================
    // STEP 1: Initialize Reverse Push Connection with Hot-Reload
    // ==========================================
    let registry_for_push = registry.clone();
    let (reconnect_tx, mut reconnect_rx) = tokio::sync::mpsc::channel::<()>(1);

    // File Watcher for config.toml
    let reconnect_tx_watcher = reconnect_tx.clone();
    rt.spawn(async move {
        let watcher_res: anyhow::Result<()> = async {
            use notify::{EventKind, RecursiveMode, Watcher};
            let config_path = wintray::config::get_config_path();
            let config_dir = config_path.parent().unwrap_or(std::path::Path::new("."));

            println!("[HotReload] Watching directory for config changes: {:?}", config_dir);

            let mut watcher =
                notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
                    if let Ok(event) = res {
                        let is_config_event =
                            event.paths.iter().any(|p| p.ends_with("config.toml"));
                        if is_config_event
                            && matches!(event.kind, EventKind::Modify(_) | EventKind::Create(_))
                        {
                            println!("[HotReload] config.toml changed, signaling reconnection...");
                            let _ = reconnect_tx_watcher.try_send(());
                        }
                    }
                })
                .context("Failed to start watcher")?;

            watcher
                .watch(config_dir, RecursiveMode::NonRecursive)
                .context("Failed to watch config directory")?;

            loop {
                tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
            }
        }
        .await;

        if let Err(e) = watcher_res {
            eprintln!("[HotReload] Watcher error: {}", e);
        }
    });

    rt.spawn(async move {
        loop {
            let config: admin::AppConfig = load_config();
            let endpoint = config.server_url.clone();
            let client_id = hardware_id::get_unique_client_id();
            let registry_task_loop = registry_for_push.clone();

            let token_display = config.jwt_token.as_deref().unwrap_or("none");
            let token_summary = if token_display.len() > 20 {
                format!("{}...{}", &token_display[..10], &token_display[token_display.len()-10..])
            } else {
                token_display.to_string()
            };

            println!("[ReversePush] Connecting to {} with token: {}...", 
                endpoint,
                token_summary
            );

            let mut push_builder = ReversePushBuilder::new()
                .endpoint(endpoint)
                .client_id(client_id)
                .with_token(config.jwt_token.clone());

            // Configure TLS
            if config.server_url.starts_with("https") {
                let mut tls_config = tonic::transport::ClientTlsConfig::new();
                let app_data = std::env::var("APPDATA").unwrap_or_else(|_| ".".to_string());
                let cert_path = std::path::PathBuf::from(app_data)
                    .join("WasmBridgeCloud")
                    .join("certs")
                    .join("cert.pem");

                if let Ok(pem) = std::fs::read_to_string(&cert_path) {
                    let cert = tonic::transport::Certificate::from_pem(pem);
                    tls_config = tls_config.ca_certificate(cert).domain_name("127.0.0.1");
                }
                push_builder = push_builder.with_tls_config(tls_config);
            }

            // Step 1: Attempt to establish connection, but be interruptible by hot-reload
            let mut push_client = tokio::select! {
                res = push_builder.build_and_run() => match res {
                    Ok(client) => client,
                    Err(e) => {
                        eprintln!("[ReversePush] Connection failed: {}. Retrying in 5s...", e);
                        tokio::select! {
                            _ = tokio::time::sleep(std::time::Duration::from_secs(5)) => {
                                continue;
                            }
                            _ = reconnect_rx.recv() => {
                        println!("[HotReload] Received signal during retry sleep. Waiting for file to settle...");
                        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                        continue;
                    }
                }
            }
        },
        _ = reconnect_rx.recv() => {
            println!("[HotReload] Received signal during connection attempt. Waiting for file to settle...");
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            continue;
        }
    };

    println!("[Router] Connected. Starting command routing loop...");

    // Step 2: Main command processing loop
    loop {
        tokio::select! {
            Some(cloud_cmd) = push_client.receive_command() => {
                if let Some(Command::ExecutePlugin(action)) = cloud_cmd.command {
                    println!("[Router] Received ExecutePlugin for '{}'", action.target_plugin);
                    let registry_task = registry_task_loop.clone();
                    let result = tokio::task::spawn_blocking(move || {
                        registry_task.execute_command(
                            &action.target_plugin,
                            &action.action,
                            &action.payload,
                        )
                    })
                    .await
                    .unwrap();
 
                    let (success, response_data, error_msg) = match result {
                        Ok(data) => (true, data, String::new()),
                        Err(e) => (false, Vec::new(), e),
                    };
 
                    let response_event = ClientEvent {
                        event: Some(
                            wasmbridge_client_proto::control_plane::client_event::Event::Response(
                                CommandResponse {
                                    command_id: cloud_cmd.command_id,
                                    success,
                                    data: response_data,
                                    error_message: error_msg,
                                },
                            ),
                        ),
                    };
                    let _ = push_client.send_event(response_event).await;
                }
            }
            _ = reconnect_rx.recv() => {
                println!("[HotReload] Received signal during active connection. Waiting for file to settle...");
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                break; // Break inner loop to restart outer loop and reload config
            }
        }
    }
        }
    });

    // ==========================================
    // STEP 2: Initialize Tray UI and Admin Web Server
    // ==========================================
    let registry_clone = registry.clone();
    let router = admin::admin_routes(registry_clone);

    WintrayAppBuilder::new()
        .with_tooltip(config.app_name)
        .with_icon(include_bytes!("../assets/tray.svg"))
        .with_assets::<admin::Assets>("/assets")
        .with_router(router)
        .with_address(address)
        .build()
        .run();

    Ok(())
}
