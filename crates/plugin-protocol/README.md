# WasmBridge Plugin Protocol

This crate defines the shared data structures and communication protocol used by the **WasmBridge** ecosystem. It provides the common language that both the WasmBridge host and its WebAssembly plugins use to exchange information.

## Overview

The protocol is designed to be lightweight and serializable via JSON (using `serde`). It covers three main areas:
1.  **Request/Response**: Standard structures for proxied HTTP traffic.
2.  **Discovery**: Metadata about plugins (endpoints, versions, configuration settings).
3.  **Reverse Push**: Protocols for executing tasks pushed from a cloud control plane to a plugin.

## Key Structures

### `PluginInfo`
Used by plugins to describe themselves to the host. It includes:
*   Name, version, and description.
*   A list of exposed `EndpointInfo` (paths and HTTP methods).
*   A list of `SettingDef` (configuration parameters required by the plugin).

### `PluginRequest` & `PluginResponse`
The standard containers for HTTP-like communication between the host's proxy and the WASM plugin.

### `CloudCommandPayload` & `CloudCommandResult`
Types used for the Reverse Push architecture, allowing remote task execution.

## Usage

This crate is typically used in:
*   **The Host**: To deserialize plugin info and wrap incoming HTTP requests.
*   **Plugins**: To implement the required interface and communicate results back to the host.

To use it in your plugin, add it to your `Cargo.toml`:

```toml
[dependencies]
plugin-protocol = { git = "https://github.com/wasmbrigde/wasmbrigde" }
```

Then implement your plugin functions using these types.
