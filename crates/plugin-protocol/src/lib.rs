//! Shared data structures and protocols for the WasmBridge plugin system.
//!
//! This crate defines the common types used for communication between the
//! WasmBridge host and its WebAssembly plugins, as well as the structures
//! for cloud-based "Reverse Push" commands.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Incoming request data sent from the host to the plugin.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PluginRequest {
    /// HTTP method (GET, POST, etc.)
    pub method: String,
    /// Path of the request
    pub path: String,
    /// HTTP headers
    pub headers: HashMap<String, String>,
    /// Query string parameters
    pub query: HashMap<String, String>,
    /// Optional binary body
    pub body: Option<Vec<u8>>,
}

/// Response from the plugin returned back to the host.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PluginResponse {
    /// HTTP status code
    pub status: u16,
    /// HTTP headers to return
    pub headers: HashMap<String, String>,
    /// Optional binary body to return
    pub body: Option<Vec<u8>>,
}

/// Plugin metadata used for discovery and configuration by the host.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PluginInfo {
    /// Name of the plugin
    pub name: String,
    /// Version string
    pub version: String,
    /// Description of what the plugin does
    pub description: String,
    /// List of supported HTTP endpoints exposed by the plugin
    pub endpoints: Vec<EndpointInfo>,
    /// List of required configuration settings for the plugin
    #[serde(default)]
    pub settings: Vec<SettingDef>,
}

/// Metadata for a single plugin endpoint.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct EndpointInfo {
    /// Path relative to the plugin's root
    pub path: String,
    /// Supported HTTP method
    pub method: String,
    /// Human-readable description of the endpoint
    pub description: String,
}

/// Supported data types for plugin settings.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum SettingType {
    /// Simple text input
    Text,
    /// Numeric input
    Number,
    /// Boolean toggle
    Boolean,
    /// Masked password input
    Password,
}

/// Definition of a configuration setting required by the plugin.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SettingDef {
    /// Unique identifier for the setting
    pub key: String,
    /// Human-readable label shown in the UI
    pub label: String,
    /// Type of the setting value
    pub setting_type: SettingType,
    /// Optional default value
    pub default_value: Option<String>,
    /// Optional help text or description
    pub description: Option<String>,
}

/// Payload structure for a command pushed from the cloud (Reverse Push).
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CloudCommandPayload {
    /// The name of the task to execute
    pub task: String,
    /// Arguments for the task
    pub args: HashMap<String, String>,
}

/// Result structure for a plugin's response to a cloud command.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CloudCommandResult {
    /// Whether the task was completed successfully
    pub success: bool,
    /// Status message or error description
    pub message: String,
    /// Optional binary payload result
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Vec<u8>>,
}
