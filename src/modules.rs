use extism::{Manifest, Plugin, Wasm, host_fn, Function, ValType, UserData};
use plugin_protocol::PluginInfo;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use serde_json;
use std::sync::OnceLock;

static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

fn get_client() -> &'static reqwest::Client {
	CLIENT.get_or_init(|| {
		reqwest::Client::builder()
			.danger_accept_invalid_certs(true)
			.build()
			.expect("Failed to build reqwest client")
	})
}
 
host_fn!(insecure_get(url: String) -> Vec<u8> {
	let client = get_client();
	
	let handle = tokio::runtime::Handle::current();
	let result = tokio::task::block_in_place(|| {
		handle.block_on(async {
			let resp = client.get(&url).send().await
				.map_err(|e| e.to_string())?;
			let bytes = resp.bytes().await
				.map_err(|e| e.to_string())?;
			Ok::<Vec<u8>, String>(bytes.to_vec())
		})
	});
	
	result.map_err(|e| extism::Error::msg(e))
});

pub struct PluginModule {
    pub info: PluginInfo,
    pub plugin: Plugin,
    pub settings: HashMap<String, String>,
    pub wasm_bytes: Vec<u8>,
}

#[derive(Clone, Default)]
pub struct ModuleRegistry {
    pub modules: Arc<RwLock<HashMap<String, Arc<RwLock<PluginModule>>>>>,
}

impl ModuleRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Загрузка всех плагинов из указанной директории
    pub fn load_all_from_dir(&self, dir_path: std::path::PathBuf) -> Result<(), String> {
        if !dir_path.exists() {
            std::fs::create_dir_all(&dir_path).map_err(|e| e.to_string())?;
        }

        for entry in std::fs::read_dir(dir_path).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            
            if path.extension().and_then(|s| s.to_str()) == Some("wasm") {
                let wasm_bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
                if let Err(e) = self.load_module(wasm_bytes) {
                    eprintln!("Failed to load plugin from {:?}: {}", path, e);
                }
            }
        }
        Ok(())
    }

    /// Инициализация плагина из байтов (WASM)
    pub fn load_module(&self, wasm_bytes: Vec<u8>) -> Result<(), String> {
        println!("load_module: received {} bytes", wasm_bytes.len());
        let name = {
            // 1. Создаем временный плагин только для получения info()
            println!("Creating temporary plugin...");
            let manifest = Manifest::new([Wasm::data(wasm_bytes.clone())])
                .with_allowed_host("*");
            let imports = [
                Function::new("insecure_get", [ValType::I64], [ValType::I64], UserData::new(()), insecure_get)
            ];
            let mut tmp_plugin = Plugin::new(&manifest, imports, true)
                .map_err(|e| format!("Failed to create temporary plugin: {}", e))?;
            
            let info_bytes = tmp_plugin.call::<&str, &[u8]>("info", "")
                .map_err(|e| format!("Failed to call info(): {}", e))?;
            
            let info: PluginInfo = serde_json::from_slice(info_bytes)
                .map_err(|e| format!("Failed to parse plugin info: {}", e))?;
            
            println!("Plugin name from info(): {}", info.name);
            info.name.clone()
        };

        // 2. Загрузка настроек из файла (Stage 2)
        let mut settings = HashMap::new();
        let mut config_path = std::env::current_exe().unwrap();
        config_path.pop();
        config_path.push("plugins");
        config_path.push(format!("{}_config.json", name));

        if config_path.exists() {
            println!("Loading settings from {:?}", config_path);
            if let Ok(content) = std::fs::read_to_string(&config_path) {
                if let Ok(loaded_settings) = serde_json::from_str::<HashMap<String, String>>(&content) {
                    settings = loaded_settings;
                    println!("Loaded {} settings", settings.len());
                }
            }
        } else {
            println!("No config file found at {:?}", config_path);
        }

        // 3. Создаем финальный плагин с загруженным конфигом (Stage 3)
        println!("Creating final plugin with config...");
        let manifest = Manifest::new([Wasm::data(wasm_bytes.clone())])
            .with_allowed_host("*")
            .with_config(settings.clone().into_iter());

        let imports = [
            Function::new("insecure_get", [ValType::I64], [ValType::I64], UserData::new(()), insecure_get)
        ];
        let mut plugin = Plugin::new(&manifest, imports, true)
            .map_err(|e| format!("Failed to create plugin with config: {}", e))?;

        // Снова получаем info (теперь у нас есть финальный плагин)
        let info_bytes = plugin.call::<&str, &[u8]>("info", "")
            .map_err(|e| format!("Failed to call info() for final plugin: {}", e))?;
        let info: PluginInfo = serde_json::from_slice(info_bytes)
            .map_err(|e| format!("Failed to parse plugin info: {}", e))?;

        // 4. Сохраняем в реестр
        let mut modules = self.modules.write().unwrap();
        modules.insert(name, Arc::new(RwLock::new(PluginModule { 
            info, 
            plugin, 
            settings,
            wasm_bytes
        })));

        Ok(())
    }

    /// Обновление настроек плагина и сохранение на диск
    pub fn update_settings(&self, name: &str, new_settings: HashMap<String, String>) -> Result<(), String> {
        let module_arc = {
            let modules = self.modules.read().unwrap();
            modules.get(name).cloned().ok_or_else(|| format!("Module {} not found", name))?
        };

        {
            let mut module = module_arc.write().unwrap();
            
            // Stage 3: В Extism 1.x конфиг задается при создании.
            // Для динамического обновления пересоздаем инстанс плагина.
            let manifest = Manifest::new([Wasm::data(module.wasm_bytes.clone())])
                .with_allowed_host("*")
                .with_config(new_settings.clone().into_iter());

            let imports = [
                Function::new("insecure_get", [ValType::I64], [ValType::I64], UserData::new(()), insecure_get)
            ];
            let new_plugin = Plugin::new(&manifest, imports, true)
                .map_err(|e| format!("Failed to recreate plugin {}: {}", name, e))?;

            module.plugin = new_plugin;
            module.settings = new_settings.clone();
        }

        // Сохранение на диск
        let mut config_path = std::env::current_exe().unwrap();
        config_path.pop();
        config_path.push("plugins");
        if !config_path.exists() {
            std::fs::create_dir_all(&config_path).map_err(|e| e.to_string())?;
        }
        config_path.push(format!("{}_config.json", name));

        let content = serde_json::to_string_pretty(&new_settings).map_err(|e| e.to_string())?;
        std::fs::write(config_path, content).map_err(|e| e.to_string())?;

        Ok(())
    }
}
