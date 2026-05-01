use askama::Template;
use axum::{
    Router,
    body::Bytes,
    extract::{Form, Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::{any, get, post},
};
use plugin_protocol::{PluginRequest, PluginResponse, SettingType};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use wintray::config::{load_config, save_config as framework_save_config};

/// Main application configuration structure.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AppConfig {
    pub app_name: String,
    pub port: u16,
    pub debug_mode: bool,
    pub server_url: String,
    pub jwt_token: Option<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            app_name: "Monitoring Bridge".to_string(),
            port: 9876,
            debug_mode: false,
            server_url: "https://127.0.0.1:50051".to_string(),
            jwt_token: Some("debug-token-123".to_string()),
        }
    }
}

/// Data used for displaying plugin information in the web UI.
#[derive(Serialize, Deserialize, Clone)]
pub struct ModuleDisplayData {
    pub info: plugin_protocol::PluginInfo,
    pub settings: HashMap<String, String>,
}

/// Template structure for the main admin dashboard page.
#[derive(Template)]
#[template(path = "index.html")]
struct IndexTemplate {
    config: AppConfig,
    modules: Vec<ModuleDisplayData>,
    hardware_id: String,
}

/// Handler for GET /: Renders the admin dashboard.
async fn render_index(State(registry): State<crate::modules::ModuleRegistry>) -> impl IntoResponse {
    let config: AppConfig = load_config();

    let modules_data = {
        let modules = match registry.modules.read() {
            Ok(m) => m,
            Err(_) => {
                return (StatusCode::INTERNAL_SERVER_ERROR, "Registry lock poisoned")
                    .into_response();
            }
        };
        modules
            .values()
            .map(|m| {
                let m = m.read().unwrap_or_else(|e| e.into_inner());
                ModuleDisplayData { info: m.info.clone(), settings: m.settings.clone() }
            })
            .collect::<Vec<_>>()
    };

    let hardware_id = crate::hardware_id::get_unique_client_id();
    let template = IndexTemplate { config, modules: modules_data, hardware_id };
    Html(template.render().unwrap()).into_response()
}

/// Handler for POST /save: Saves the global application configuration.
async fn save_config(Form(config): Form<AppConfig>) -> impl IntoResponse {
    match framework_save_config(&config) {
        Ok(_) => {
            Html("<span style='color: green;'>Configuration saved successfully!</span>".to_string())
        }
        Err(e) => Html(format!("<span style='color: red;'>Error saving configuration: {}</span>", e)),
    }
}

/// Generic dispatcher that routes HTTP requests to specific plugins.
async fn dispatch_request(
    State(registry): State<crate::modules::ModuleRegistry>,
    method: axum::http::Method,
    headers: HeaderMap,
    Path((module_name, subpath)): Path<(String, String)>,
    Query(query): Query<HashMap<String, String>>,
    body: Bytes,
) -> impl IntoResponse {
    // 1. Locate the requested plugin in the registry.
    let plugin_arc = {
        let modules = match registry.modules.read() {
            Ok(m) => m,
            Err(_) => {
                return (StatusCode::INTERNAL_SERVER_ERROR, "Registry lock poisoned")
                    .into_response();
            }
        };
        match modules.get(&module_name) {
            Some(p) => p.clone(),
            None => {
                return (StatusCode::NOT_FOUND, format!("Module '{}' not found", module_name))
                    .into_response();
            }
        }
    };

    // 2. Prepare the PluginRequest container.
    let mut plugin_headers = HashMap::new();
    for (name, value) in headers.iter() {
        if let Ok(val_str) = value.to_str() {
            plugin_headers.insert(name.to_string(), val_str.to_string());
        }
    }

    let plugin_req = PluginRequest {
        method: method.to_string(),
        path: format!("/{}", subpath),
        headers: plugin_headers,
        query,
        body: if body.is_empty() { None } else { Some(body.to_vec()) },
    };

    // 3. Execute the plugin's handle_request function.
    let mut module = match plugin_arc.write() {
        Ok(m) => m,
        Err(_) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, "Plugin lock poisoned").into_response();
        }
    };
    let result = module.plugin.call::<&[u8], &[u8]>(
        "handle_request",
        &serde_json::to_vec(&plugin_req).unwrap_or_default(),
    );

    match result {
        Ok(resp_bytes) => {
            let plugin_resp: PluginResponse = match serde_json::from_slice(resp_bytes) {
                Ok(r) => r,
                Err(e) => {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("Plugin returned invalid JSON: {}", e),
                    )
                        .into_response();
                }
            };

            // Convert PluginResponse into an axum::response::Response.
            let mut response_builder = Response::builder().status(
                StatusCode::from_u16(plugin_resp.status)
                    .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            );

            for (k, v) in plugin_resp.headers {
                response_builder = response_builder.header(k, v);
            }

            let body_bytes = plugin_resp.body.unwrap_or_default();
            response_builder.body(axum::body::Body::from(body_bytes)).unwrap().into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("Plugin execution error: {}", e))
            .into_response(),
    }
}

use axum::extract::Multipart;

/// Handler for uploading a new WASM module file.
async fn upload_module(
    State(registry): State<crate::modules::ModuleRegistry>,
    mut multipart: Multipart,
) -> impl IntoResponse {
    while let Ok(Some(field)) = multipart.next_field().await {
        let name = field.name().unwrap_or_default().to_string();
        let file_name = field.file_name().unwrap_or_default().to_string();
        println!("Uploading module field: {}, file: {}", name, file_name);

        if name == "plugin" && file_name.ends_with(".wasm") {
            let data = match field.bytes().await {
                Ok(b) => b.to_vec(),
                Err(e) => {
                    return (StatusCode::BAD_REQUEST, format!("Failed to read file: {}", e))
                        .into_response();
                }
            };

            // 1. Attempt to load the module into memory for validation.
            println!("Validating module...");
            if let Err(e) = registry.load_module(data.clone()) {
                println!("Validation failed: {}", e);
                return (StatusCode::BAD_REQUEST, format!("Invalid WASM module: {}", e))
                    .into_response();
            }
            println!("Module validated successfully.");

            // 2. If valid, save to the plugins directory for persistence.
            let mut path = std::env::current_exe().unwrap();
            path.pop();
            path.push("plugins");

            if !path.exists() {
                println!("Creating plugins directory: {:?}", path);
                if let Err(e) = std::fs::create_dir_all(&path) {
                    println!("Failed to create plugins directory: {}", e);
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("Failed to create plugins directory: {}", e),
                    )
                        .into_response();
                }
            }

            path.push(&file_name);

            if let Err(e) = std::fs::write(&path, data) {
                println!("Failed to save module to disk: {}", e);
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to save module to disk: {}", e),
                )
                    .into_response();
            }
            println!("Module saved to {:?}", path);

            return (
                StatusCode::OK,
                [("HX-Refresh", "true")],
                format!("Module '{}' successfully uploaded and activated", file_name),
            )
                .into_response();
        }
    }

    (StatusCode::BAD_REQUEST, "No valid .wasm file found in 'plugin' field").into_response()
}

/// Handler for POST /admin/modules/:module_name/settings: Saves module-specific configuration.
async fn save_module_settings(
    State(registry): State<crate::modules::ModuleRegistry>,
    Path(module_name): Path<String>,
    Form(settings): Form<HashMap<String, String>>,
) -> impl IntoResponse {
    match registry.update_settings(&module_name, settings) {
        Ok(_) => Html("<span style='color: green;'>Module settings saved successfully!</span>".to_string()),
        Err(e) => Html(format!("<span style='color: red;'>Error saving settings: {}</span>", e)),
    }
}

/// Configures all routes for the admin interface and plugin proxy.
pub fn admin_routes(registry: crate::modules::ModuleRegistry) -> Router {
    Router::new()
        .route("/", get(render_index))
        .route("/save", post(save_config))
        .route("/assets/{*path}", get(crate::assets::static_handler))
        .route("/api/modules/{module_name}/{*subpath}", any(dispatch_request))
        .route("/admin/modules/upload", post(upload_module))
        .route("/admin/modules/{module_name}/settings", post(save_module_settings))
        .with_state(registry)
}
