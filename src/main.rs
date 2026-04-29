mod admin;
mod assets;
mod modules;

use wintray::config::load_config;
use wintray::WintrayAppBuilder;

#[cfg(not(windows))]
compile_error!("Этот проект поддерживает только Windows.");

#[cfg(windows)]
fn main() {
    let registry = modules::ModuleRegistry::new();

    // Загружаем плагины из AppData
    let mut plugins_dir = std::env::var("APPDATA")
        .map(std::path::PathBuf::from)
        .expect("AppData directory not found");
    plugins_dir.push("WasmBridge");
    plugins_dir.push("plugins");

    if let Err(e) = registry.load_all_from_dir(plugins_dir) {
        eprintln!("Error loading initial plugins: {}", e);
    }

    // Загружаем конфиг для получения порта
    let config: admin::AppConfig = load_config();
    let address = format!("127.0.0.1:{}", config.port);

    let registry_clone = registry.clone();
    let router = admin::admin_routes(registry_clone);

    let app = WintrayAppBuilder::new()
        .with_tooltip(config.app_name)
        .with_icon(include_bytes!("../assets/tray.svg"))
        .with_router(router)
        .with_address(address)
        .add_menu_item("info", "О плагинах")
        .build();

    app.run_with(|menu_id| {
        if menu_id == "info" {
            println!("Пользователь нажал 'О плагинах'");
        }
    });
}
