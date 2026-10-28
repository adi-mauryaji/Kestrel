//! The devices llama.cpp can run a model on (v4 A2's probe, ADR-053), as plain data: filled by
//! `llama::backend_devices` in a llama build, made up in tests.

/// One of llama.cpp's backend devices. The CPU's memory is the machine's RAM (its free memory is
/// right on Windows only: llama.cpp reports free = total elsewhere).
#[derive(Clone, Debug, PartialEq)]
pub struct BackendDevice {
    /// "CPU", "Vulkan", "Metal", ...
    pub backend: String,
    /// "AMD Ryzen 5 5500U", "NVIDIA GeForce RTX 4060", "Apple M4", ...
    pub description: String,
    pub kind: DeviceKind,
    /// Bytes.
    pub memory_total: u64,
    pub memory_free: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceKind {
    Cpu,
    Gpu,
    IntegratedGpu,
    Accelerator,
    Unknown,
}
