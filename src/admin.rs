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

// Структура конфигурации. Поля должны совпадать с атрибутами name в HTML-форме.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AppConfig {
    pub app_name: String,
    pub port: u16,
    pub debug_mode: bool,
}

// Дефолтный конфиг на случай отсутствия файла
impl Default for AppConfig {
    fn default() -> Self {
        Self { app_name: "Monitoring Bridge".to_string(), port: 9876, debug_mode: false }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ModuleDisplayData {
    pub info: plugin_protocol::PluginInfo,
    pub settings: HashMap<String, String>,
}

// Привязка структуры к HTML-шаблону
#[derive(Template)]
#[template(path = "index.html")]
struct IndexTemplate {
    config: AppConfig,
    modules: Vec<ModuleDisplayData>,
}

// Обработчик GET: отдает страницу с заполненной формой
async fn render_index(State(registry): State<crate::modules::ModuleRegistry>) -> impl IntoResponse {
    let config: AppConfig = load_config();

    let modules_data = {
        let modules = registry.modules.read().unwrap();
        modules
            .values()
            .map(|m| {
                let m = m.read().unwrap();
                ModuleDisplayData { info: m.info.clone(), settings: m.settings.clone() }
            })
            .collect::<Vec<_>>()
    };

    let template = IndexTemplate { config, modules: modules_data };
    Html(template.render().unwrap())
}

// Обработчик POST: сохраняет конфиг и возвращает сообщение
async fn save_config(Form(config): Form<AppConfig>) -> impl IntoResponse {
    match framework_save_config(&config) {
        Ok(_) => {
            Html("<span style='color: green;'>Конфигурация успешно сохранена!</span>".to_string())
        }
        Err(e) => Html(format!("<span style='color: red;'>Ошибка сохранения: {}</span>", e)),
    }
}

async fn dispatch_request(
    State(registry): State<crate::modules::ModuleRegistry>,
    method: axum::http::Method,
    headers: HeaderMap,
    Path((module_name, subpath)): Path<(String, String)>,
    Query(query): Query<HashMap<String, String>>,
    body: Bytes,
) -> impl IntoResponse {
    // 1. Поиск плагина в реестре
    let plugin_arc = {
        let modules = registry.modules.read().unwrap();
        match modules.get(&module_name) {
            Some(p) => p.clone(),
            None => {
                return (StatusCode::NOT_FOUND, format!("Module '{}' not found", module_name))
                    .into_response();
            }
        }
    };

    // 2. Подготовка запроса для плагина
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

    // 3. Вызов плагина
    let mut module = plugin_arc.write().unwrap();
    let result = module
        .plugin
        .call::<&[u8], &[u8]>("handle_request", &serde_json::to_vec(&plugin_req).unwrap());

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

            // Конвертация PluginResponse в axum::response::Response
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

// Обработчик загрузки нового модуля
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

            // 1. Пытаемся загрузить модуль в память (валидация)
            println!("Validating module...");
            if let Err(e) = registry.load_module(data.clone()) {
                println!("Validation failed: {}", e);
                return (StatusCode::BAD_REQUEST, format!("Invalid WASM module: {}", e))
                    .into_response();
            }
            println!("Module validated successfully.");

            // 2. Если валидация прошла, сохраняем на диск для персистентности
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

// Обработчик POST: сохраняет настройки конкретного модуля
async fn save_module_settings(
    State(registry): State<crate::modules::ModuleRegistry>,
    Path(module_name): Path<String>,
    Form(settings): Form<HashMap<String, String>>,
) -> impl IntoResponse {
    match registry.update_settings(&module_name, settings) {
        Ok(_) => Html("<span style='color: green;'>Настройки модуля сохранены!</span>".to_string()),
        Err(e) => Html(format!("<span style='color: red;'>Ошибка: {}</span>", e)),
    }
}

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
