mod admin;
mod assets;
mod hardware_id;
mod modules;

use reverse_push::ReversePushBuilder;
use reverse_push::control_plane::{ClientEvent, CommandResponse, cloud_command::Command};
use wintray::WintrayAppBuilder;
use wintray::config::load_config;

#[cfg(not(windows))]
compile_error!("WasmBridge currently only supports Windows.");

#[cfg(windows)]
fn main() {
    // Initialize the shared plugin registry
    let registry = modules::ModuleRegistry::new();

    // Determine the directory for plugins (AppData/WasmBridge/plugins)
    let mut plugins_dir = std::env::var("APPDATA")
        .map(std::path::PathBuf::from)
        .expect("AppData directory not found");
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
    let rt = tokio::runtime::Runtime::new().unwrap();
    let _guard = rt.enter();

    // Explicitly install the cryptography provider for rustls (required in 0.23+)
    rustls::crypto::ring::default_provider().install_default().ok();

    // ==========================================
    // STEP 1: Initialize Reverse Push Connection
    // ==========================================
    let registry_for_push = registry.clone();

    rt.spawn(async move {
        let endpoint = config.server_url.clone();
        let client_id = hardware_id::get_unique_client_id();

        let mut push_builder = ReversePushBuilder::new()
            .endpoint(endpoint)
            .client_id(client_id)
            .with_token(config.jwt_token.clone());

        // Configure TLS for secure communication with the Cloud Control Plane
        if config.server_url.starts_with("https") {
            let mut tls_config = tonic::transport::ClientTlsConfig::new();

            #[cfg(debug_assertions)]
            {
                // In debug mode, load the local self-signed certificate if available
                let app_data = std::env::var("APPDATA").unwrap_or_else(|_| ".".to_string());
                let cert_path = std::path::PathBuf::from(app_data)
                    .join("WasmBridgeCloud")
                    .join("certs")
                    .join("cert.pem");

                if let Ok(pem) = std::fs::read_to_string(&cert_path) {
                    let cert = tonic::transport::Certificate::from_pem(pem);
                    tls_config = tls_config.ca_certificate(cert).domain_name("127.0.0.1");
                } else {
                    eprintln!("[ReversePush] Warning: cert.pem not found at {:?}", cert_path);
                }
            }

            push_builder = push_builder.with_tls_config(tls_config);
        }

        // Build and start the background connection loop
        let mut push_client =
            push_builder.build_and_run().await.expect("Failed to start reverse push client");

        println!("[Router] Starting cloud command routing loop...");

        // Main loop for handling commands pushed from the cloud
        while let Some(cloud_cmd) = push_client.receive_command().await {
            if let Some(command) = cloud_cmd.command {
                match command {
                    Command::ExecutePlugin(action) => {
                        println!(
                            "[Router] Received ExecutePlugin for '{}' action '{}'",
                            action.target_plugin, action.action
                        );

                        let registry_task = registry_for_push.clone();
                        let action_task = action.clone();

                        // Execute the plugin function in a blocking thread (safe for WASM execution)
                        let result = tokio::task::spawn_blocking(move || {
                            registry_task.execute_command(
                                &action_task.target_plugin,
                                &action_task.action,
                                &action_task.payload,
                            )
                        })
                        .await
                        .unwrap();

                        let (success, response_data, error_msg) = match result {
                            Ok(data) => (true, data, String::new()),
                            Err(e) => {
                                eprintln!("[Router] Plugin error: {}", e);
                                (false, Vec::new(), e)
                            }
                        };

                        // Send the execution result back to the Cloud Control Plane
                        let response_event = ClientEvent {
                            event: Some(
                                reverse_push::control_plane::client_event::Event::Response(
                                    CommandResponse {
                                        command_id: cloud_cmd.command_id,
                                        success,
                                        data: response_data,
                                        error_message: error_msg,
                                    },
                                ),
                            ),
                        };

                        if let Err(e) = push_client.send_event(response_event).await {
                            eprintln!("[Router] Failed to send response: {}", e);
                        }
                    }
                    Command::UpdateConfig(_) => {
                        println!("[Router] UpdateConfig not implemented yet");
                    }
                    Command::Restart(_) => {
                        println!("[Router] Restart not implemented yet");
                    }
                }
            }
        }
        println!("[Router] Command routing loop terminated.");
    });

    // ==========================================
    // STEP 2: Initialize Tray UI and Admin Web Server
    // ==========================================
    let registry_clone = registry.clone();
    let router = admin::admin_routes(registry_clone);

    let app = WintrayAppBuilder::new()
        .with_tooltip(config.app_name)
        .with_icon(include_bytes!("../assets/tray.svg"))
        .with_router(router)
        .with_address(address)
        .build()
		.run();
}
