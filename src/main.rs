mod admin;
mod assets;
mod modules;

use tao::event_loop::ControlFlow;
use wintray::config::load_config;
use wintray::engine::ServiceEngine;
use wintray::tray::{TrayConfig, TrayUserEvent};

#[cfg(not(windows))]
compile_error!("Этот проект поддерживает только Windows.");

#[cfg(windows)]
fn main() {
	let registry = modules::ModuleRegistry::new();

	// Загружаем плагины
	let mut plugins_dir = std::env::current_exe().expect("Failed to get exe path");
	plugins_dir.pop();
	plugins_dir.push("plugins");

	if let Err(e) = registry.load_all_from_dir(plugins_dir) {
		eprintln!("Error loading initial plugins: {}", e);
	}

	// Загружаем конфиг для получения порта
	let config: admin::AppConfig = load_config();
	let address = format!("127.0.0.1:{}", config.port);
	let ui_address = address.clone(); // Для использования в замыканиях

	let registry_clone = registry.clone();
	let router = admin::admin_routes(registry_clone);

	let tray_config = TrayConfig {
		tooltip: config.app_name,
		icon_svg_bytes: include_bytes!("../assets/tray.svg"),
		custom_menu_items: vec![("info".to_string(), "О плагинах".to_string())],
	};

	let engine = ServiceEngine::new(tray_config, router, address);

	engine.run(move |user_event, _proxy, control_flow| match user_event {
		TrayUserEvent::TrayIconEvent(tray_event) => {
			if let tray_icon::TrayIconEvent::Click {
				button: tray_icon::MouseButton::Left,
				button_state: tray_icon::MouseButtonState::Up,
				..
			} = tray_event
			{
				let _ = open::that(format!("http://{}", ui_address));
			}
		}
		TrayUserEvent::MenuEvent(menu_event) => {
			if menu_event.id == "open" {
				let _ = open::that(format!("http://{}", ui_address));
			} else if menu_event.id == "info" {
				println!("Пользователь нажал 'О плагинах'");
			} else if menu_event.id == "close" {
				*control_flow = ControlFlow::Exit;
			}
		}
	});
}
