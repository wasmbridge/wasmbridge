use sha2::{Digest, Sha256};
use sysinfo::System;

/// Generates a human-readable unique identifier for the current host.
///
/// The ID is composed of the system hostname and a short hash of
/// hardware parameters (CPU, OS, Total Memory) to ensure uniqueness
/// across different machines.
pub fn get_unique_client_id() -> String {
    let hostname = System::host_name().unwrap_or_else(|| "unknown-host".to_string());
    let hash = generate_hardware_hash();

    // Take first 8 chars of hash for brevity in the ID, while keeping uniqueness high
    format!("{}-{}", hostname, &hash[..8])
}

/// Generates a semi-stable hash based on the machine's hardware profile.
pub fn generate_hardware_hash() -> String {
    let mut sys = System::new_all();
    sys.refresh_all();

    // 1. Hostname, OS name and version
    let host_name = System::host_name().unwrap_or_else(|| "Unknown".to_string());
    let os_name = System::name().unwrap_or_else(|| "Unknown".to_string());
    let os_version = System::os_version().unwrap_or_else(|| "Unknown".to_string());

    // 2. CPU info
    let cpus = sys.cpus();
    let cpu_info =
        if !cpus.is_empty() { cpus[0].brand().to_string() } else { "UnknownCPU".to_string() };

    // 3. Total RAM
    let total_memory = sys.total_memory();

    // 4. Raw data string for hashing
    let raw_hardware_string =
        format!("{}-{}-{}-{}-{}", host_name, os_name, os_version, cpu_info, total_memory);

    // 5. SHA256 Hash
    let mut hasher = Sha256::new();
    hasher.update(raw_hardware_string.as_bytes());
    let result = hasher.finalize();

    hex::encode(result)
}
