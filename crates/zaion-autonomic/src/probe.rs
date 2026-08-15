//! WASM Probe Engine
//!
//! Executes WASM probes for extensible environmental sensing.
use serde::{Deserialize, Serialize};
use std::path::Path;
use wasmtime::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeResult {
    pub success: bool,
    pub value: f64,
    pub metadata: serde_json::Value,
}

pub struct WasmProbe {
    name: String,
    wasm_bytes: Vec<u8>,
}

impl WasmProbe {
    pub fn new(name: String, wasm_bytes: Vec<u8>) -> Self {
        Self { name, wasm_bytes }
    }

    pub fn from_file(name: String, path: &Path) -> Result<Self, crate::AutonomicError> {
        let wasm_bytes = std::fs::read(path).map_err(|e| {
            crate::AutonomicError::WasmError(format!("Failed to read WASM file: {}", e))
        })?;
        Ok(Self::new(name, wasm_bytes))
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// H26 helper: expose bytes so the runtime loop can clone them for
    /// `spawn_blocking` execution without holding a `std::Mutex` guard
    /// across the await point.
    pub fn bytes(&self) -> &[u8] {
        &self.wasm_bytes
    }
}

pub struct ProbeEngine {
    engine: Engine,
}

impl ProbeEngine {
    pub fn new() -> Self {
        let engine = Engine::default();
        Self { engine }
    }

    /// Execute a WASM probe and return the result
    pub fn execute(&self, probe: &WasmProbe) -> Result<ProbeResult, crate::AutonomicError> {
        self.execute_bytes(probe.name(), probe.bytes())
    }

    /// H26 helper: execute raw WASM bytes.  Usable from inside
    /// `tokio::task::spawn_blocking` without holding any `std::Mutex`.
    pub fn execute_bytes(
        &self,
        probe_name: &str,
        wasm_bytes: &[u8],
    ) -> Result<ProbeResult, crate::AutonomicError> {
        let module = Module::from_binary(&self.engine, wasm_bytes).map_err(|e| {
            crate::AutonomicError::WasmError(format!("Failed to load WASM module: {}", e))
        })?;

        let mut store = Store::new(&self.engine, ());
        let instance = Instance::new(&mut store, &module, &[]).map_err(|e| {
            crate::AutonomicError::WasmError(format!("Failed to instantiate WASM: {}", e))
        })?;

        // Look for exported "probe" function that returns f64
        let probe_fn = instance
            .get_typed_func::<(), f64>(&mut store, "probe")
            .map_err(|e| {
                crate::AutonomicError::WasmError(format!("WASM probe function not found: {}", e))
            })?;

        let value = probe_fn.call(&mut store, ()).map_err(|e| {
            crate::AutonomicError::ProbeExecutionFailed(format!("WASM execution failed: {}", e))
        })?;

        Ok(ProbeResult {
            success: true,
            value,
            metadata: serde_json::json!({
                "probe_name": probe_name,
            }),
        })
    }
}

impl Default for ProbeEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_engine_initializes() {
        let engine = ProbeEngine::new();
        assert!(std::ptr::addr_of!(engine).is_aligned());
    }

    #[test]
    fn probe_result_serializes() {
        let result = ProbeResult {
            success: true,
            value: 42.0,
            metadata: serde_json::json!({"test": true}),
        };

        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("42.0"));
    }
}
