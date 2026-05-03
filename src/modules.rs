use extism::{Function, Manifest, Plugin, UserData, ValType, Wasm, host_fn};
use plugin_protocol::PluginInfo;
use std::collections::HashMap;
use std::sync::OnceLock;
use std::sync::{Arc, RwLock};

static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

/// Provides a global reqwest client for host functions.
fn get_client() -> &'static reqwest::Client {
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .danger_accept_invalid_certs(true)
            .build()
            .expect("Failed to build reqwest client")
    })
}

/// Host function: Allows plugins to perform insecure HTTP GET requests.
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

    result.map_err(extism::Error::msg)
});

/// Host function: Allows plugins to get the current system date and time.
host_fn!(get_date() -> String {
    let now = chrono::Local::now();
    Ok(now.format("%Y-%m-%d %H:%M:%S").to_string())
});

/// Represents a loaded WebAssembly plugin module.
pub struct PluginModule {
    /// Metadata about the plugin (name, endpoints, etc.)
    pub info: PluginInfo,
    /// The actual Extism plugin instance.
    pub plugin: Plugin,
    /// Current configuration settings for this module.
    pub settings: HashMap<String, String>,
    /// Raw WASM bytes (used for re-initializing the plugin with new config).
    pub wasm_bytes: Vec<u8>,
}

/// Thread-safe registry for managing all loaded plugin modules.
#[derive(Clone, Default)]
pub struct ModuleRegistry {
    /// Map of plugin names to their module instances.
    pub modules: Arc<RwLock<HashMap<String, Arc<RwLock<PluginModule>>>>>,
}

impl ModuleRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Recursively loads all `.wasm` files from the specified directory.
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

    /// Initializes a plugin from WASM bytes.
    /// This involves a multi-stage loading process to extract metadata and apply configuration.
    pub fn load_module(&self, wasm_bytes: Vec<u8>) -> Result<(), String> {
        println!("load_module: received {} bytes", wasm_bytes.len());
        let name = {
            // Stage 1: Create a temporary plugin instance to call info() and get the plugin name.
            println!("Creating temporary plugin...");
            let manifest = Manifest::new([Wasm::data(wasm_bytes.clone())]).with_allowed_host("*");
            let imports = [
                Function::new(
                    "insecure_get",
                    [ValType::I64],
                    [ValType::I64],
                    UserData::new(()),
                    insecure_get,
                ),
                Function::new("get_date", [], [ValType::I64], UserData::new(()), get_date),
            ];
            let mut tmp_plugin = Plugin::new(&manifest, imports, true)
                .map_err(|e| format!("Failed to create temporary plugin: {}", e))?;

            let info_bytes = tmp_plugin
                .call::<&str, &[u8]>("info", "")
                .map_err(|e| format!("Failed to call info(): {}", e))?;

            let info: PluginInfo = serde_json::from_slice(info_bytes)
                .map_err(|e| format!("Failed to parse plugin info: {}", e))?;

            println!("Plugin name from info(): {}", info.name);
            info.name.clone()
        };

        // Stage 2: Load existing settings from disk if available.
        let mut settings = HashMap::new();
        let mut config_path = std::env::var("APPDATA")
            .map(std::path::PathBuf::from)
            .expect("AppData directory not found");
        config_path.push("WasmBridge");
        config_path.push("plugins");
        config_path.push(format!("{}_config.json", name));

        if config_path.exists() {
            println!("Loading settings from {:?}", config_path);
            if let Ok(content) = std::fs::read_to_string(&config_path)
                && let Ok(loaded_settings) =
                    serde_json::from_str::<HashMap<String, String>>(&content)
            {
                settings = loaded_settings;
                println!("Loaded {} settings", settings.len());
            }
        }

        // Stage 3: Create the final plugin instance with the loaded configuration.
        println!("Creating final plugin with config...");
        let manifest = Manifest::new([Wasm::data(wasm_bytes.clone())])
            .with_allowed_host("*")
            .with_config(settings.clone().into_iter());
        let imports = [
            Function::new(
                "insecure_get",
                [ValType::I64],
                [ValType::I64],
                UserData::new(()),
                insecure_get,
            ),
            Function::new("get_date", [], [ValType::I64], UserData::new(()), get_date),
        ];

        let mut plugin = Plugin::new(&manifest, imports, true)
            .map_err(|e| format!("Failed to create plugin with config: {}", e))?;

        // Re-call info() to get any dynamically updated metadata.
        let info_bytes = plugin
            .call::<&str, &[u8]>("info", "")
            .map_err(|e| format!("Failed to call info() for final plugin: {}", e))?;
        let info: PluginInfo = serde_json::from_slice(info_bytes)
            .map_err(|e| format!("Failed to parse plugin info: {}", e))?;

        // Stage 4: Store the module in the registry.
        let mut modules = self
            .modules
            .write()
            .map_err(|e| format!("Poisoned lock on modules registry: {}", e))?;
        modules.insert(
            name,
            Arc::new(RwLock::new(PluginModule { info, plugin, settings, wasm_bytes })),
        );

        Ok(())
    }

    /// Updates the settings for a plugin, re-initializes its instance, and saves to disk.
    pub fn update_settings(
        &self,
        name: &str,
        new_settings: HashMap<String, String>,
    ) -> Result<(), String> {
        let module_arc = {
            let modules = self
                .modules
                .read()
                .map_err(|e| format!("Poisoned lock on modules registry: {}", e))?;
            modules.get(name).cloned().ok_or_else(|| format!("Module {} not found", name))?
        };

        {
            let mut module = module_arc
                .write()
                .map_err(|e| format!("Poisoned lock on module {}: {}", name, e))?;

            // In Extism 1.x, config is immutable after creation.
            // To update it dynamically, we recreate the plugin instance.
            let manifest = Manifest::new([Wasm::data(module.wasm_bytes.clone())])
                .with_allowed_host("*")
                .with_config(new_settings.clone().into_iter());

            let imports = [
                Function::new(
                    "insecure_get",
                    [ValType::I64],
                    [ValType::I64],
                    UserData::new(()),
                    insecure_get,
                ),
                Function::new("get_date", [], [ValType::I64], UserData::new(()), get_date),
            ];
            let new_plugin = Plugin::new(&manifest, imports, true)
                .map_err(|e| format!("Failed to recreate plugin {}: {}", name, e))?;

            module.plugin = new_plugin;
            module.settings = new_settings.clone();
        }

        // Save new settings to the AppData configuration file.
        let mut config_path = std::env::var("APPDATA")
            .map(std::path::PathBuf::from)
            .expect("AppData directory not found");
        config_path.push("WasmBridge");
        config_path.push("plugins");

        if !config_path.exists() {
            std::fs::create_dir_all(&config_path).map_err(|e| e.to_string())?;
        }
        config_path.push(format!("{}_config.json", name));

        let content = serde_json::to_string_pretty(&new_settings).map_err(|e| e.to_string())?;
        std::fs::write(config_path, content).map_err(|e| e.to_string())?;

        Ok(())
    }

    /// Executes an exported function inside a plugin.
    /// Used for routing remote commands from the Cloud Control Plane.
    pub fn execute_command(
        &self,
        name: &str,
        action: &str,
        payload: &[u8],
    ) -> Result<Vec<u8>, String> {
        let module_arc = {
            let modules = self
                .modules
                .read()
                .map_err(|e| format!("Poisoned lock on modules registry: {}", e))?;
            modules.get(name).cloned().ok_or_else(|| format!("Module {} not found", name))?
        };

        let mut module =
            module_arc.write().map_err(|e| format!("Poisoned lock on module {}: {}", name, e))?;

        let result_bytes = module
            .plugin
            .call::<&[u8], &[u8]>(action, payload)
            .map_err(|e| format!("Plugin execution failed: {}", e))?;

        Ok(result_bytes.to_vec())
    }
}
