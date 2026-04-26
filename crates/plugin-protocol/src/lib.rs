use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Данные входящего запроса от Системы А к плагину
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PluginRequest {
    pub method: String,
    pub path: String,
    pub headers: HashMap<String, String>,
    pub query: HashMap<String, String>,
    pub body: Option<Vec<u8>>,
}

/// Ответ плагина, который будет возвращен Системе А
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PluginResponse {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: Option<Vec<u8>>,
}

/// Метаданные плагина для системы обнаружения
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PluginInfo {
    pub name: String,
    pub version: String,
    pub description: String,
    /// Список поддерживаемых эндпоинтов для Системы А
    pub endpoints: Vec<EndpointInfo>,
    /// Список требуемых настроек для плагина
    #[serde(default)]
    pub settings: Vec<SettingDef>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct EndpointInfo {
    pub path: String,
    pub method: String,
    pub description: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum SettingType {
    Text,
    Number,
    Boolean,
    Password,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SettingDef {
    pub key: String,
    pub label: String,
    pub setting_type: SettingType,
    pub default_value: Option<String>,
    pub description: Option<String>,
}
