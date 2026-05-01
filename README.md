# WasmBridge

WasmBridge is a powerful, plugin-based agent architecture designed to bridge the gap between local system management and cloud control planes. It allows you to run secure, sandboxed WebAssembly (WASM) plugins on local Windows machines and interact with them via a web UI or remotely through a cloud-based gRPC control plane.

## Key Features

*   **Secure Plugin Execution**: Uses [Extism](https://extism.org/) to run WASM plugins in a restricted sandbox.
*   **Reverse Push Architecture**: Maintains a persistent outbound gRPC stream to the cloud, allowing the cloud to "push" commands to the agent even behind NAT or firewalls.
*   **Dynamic Configuration**: Manage plugin settings and global application config via a built-in web dashboard.
*   **Hot-Reloading**: Upload and activate new plugins without restarting the agent.
*   **Windows Tray Integration**: Runs as a lightweight system tray application using the `wintray` framework.
*   **Host Functions**: Provides plugins with secure access to host capabilities (e.g., date/time, local network requests).

## Repository Structure

*   `src/`: Main agent source code (registry, admin UI, routing logic).
*   `crates/`:
    *   `plugin-protocol/`: Shared data structures used by host and plugins.
*   `libs/`:
    *   `reverse_push/`: Client library for bi-directional gRPC communication.
    *   `wintray/`: Windows system tray and web UI framework.
    *   `wasmbrigde-plugin-template/`: A starting point for creating your own plugins.

## Getting Started

### Prerequisites

*   Rust 1.75+ (edition 2024)
*   Windows (main agent only supports Windows)

### Building the Agent

```bash
cargo build --release
```

### Running the Agent

The agent will start in the system tray. Right-click the icon and select **Open UI** to access the dashboard. By default, the dashboard is available at `https://127.0.0.1:9876`.

## Plugin Development

To create a new plugin:
1.  Copy the `libs/wasmbrigde-plugin-template` directory.
2.  Implement the `info()`, `handle_request()`, and/or `execute_command()` functions.
3.  Compile to `wasm32-unknown-unknown`.
4.  Upload the `.wasm` file via the WasmBridge dashboard.

Refer to the [Plugin Template README](./libs/wasmbrigde-plugin-template/README.md) for more details.

## License

MIT
