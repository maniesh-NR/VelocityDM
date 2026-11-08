// SPDX-License-Identifier: GPL-3.0-or-later
//! The RPC surface implemented over the vdm-core manager.

use serde_json::{json, Value};
use vdm_core::manager::Manager;
use vdm_core::VdmError;
use vdm_rpc::{RpcError, RpcHandler};

fn daemon_err(e: VdmError) -> RpcError {
    RpcError::new(vdm_rpc::codes::DAEMON, e.to_string())
}

pub struct DaemonService {
    pub manager: std::sync::Arc<Manager>,
    pub version: String,
    pub shutdown_flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl RpcHandler for DaemonService {
    fn call(&self, method: &str, params: &Value) -> Result<Value, RpcError> {
        self.dispatch(method, params)
    }
}

impl DaemonService {
    fn dispatch(&self, method: &str, params: &Value) -> Result<Value, RpcError> {
        let manager = &self.manager;
        match method {
            "ping" | "hello" => Ok(json!({
                "name": "velocitydm",
                "version": self.version,
                "protocol": vdm_rpc::PROTOCOL_VERSION,
            })),
            "add" => {
                let mut spec: vdm_core::TaskSpec = serde_json::from_value(params.clone())
                    .map_err(|e| RpcError::new(vdm_rpc::codes::INVALID_PARAMS, format!("bad add params: {}", e)))?;
                if spec.url.is_empty() {
                    return Err(RpcError::new(vdm_rpc::codes::INVALID_PARAMS, "url is required"));
                }
                // Resume-friendly default: pick up any .part.vdmctl.
                spec.resume = params.get("resume").and_then(|v| v.as_bool()).unwrap_or(true);
                let id = futures_block_on_add(manager, spec)?;
                Ok(json!({ "id": id }))
            }
            "pause" => {
                let id = parse_id(params)?;
                let state: vdm_core::TaskState = block_on(manager.pause(id)).map_err(daemon_err)?;
                Ok(json!({ "id": id, "state": state.as_str() }))
            }
            "resume" => {
                let id = parse_id(params)?;
                let state = manager.resume(id).map_err(daemon_err)?;
                Ok(json!({ "id": id, "state": state.as_str() }))
            }
            "remove" => {
                let id = parse_id(params)?;
                let purge = params.get("purge").and_then(|v| v.as_bool()).unwrap_or(false);
                manager.remove(id, purge).map_err(daemon_err)?;
                Ok(json!({ "id": id, "removed": true }))
            }
            "list" => {
                let snaps = manager.list();
                serde_json::to_value(snaps).map_err(|e| RpcError::internal(e.to_string()))
            }
            "status" => {
                let id = parse_id(params)?;
                let snap = manager.get(id).ok_or_else(|| RpcError::new(vdm_rpc::codes::DAEMON, format!("no such task: {}", id)))?;
                serde_json::to_value(snap).map_err(|e| RpcError::internal(e.to_string()))
            }
            "verify" => {
                let id = parse_id(params)?;
                let ok = manager.verify(id).map_err(daemon_err)?;
                Ok(json!({ "id": id, "ok": ok }))
            }
            "stats" => {
                let stats = manager.stats();
                let (connects, reuses) = manager.env().client.pool().stats();
                Ok(json!({
                    "tasks": stats.tasks,
                    "running": stats.running,
                    "queued": stats.queued,
                    "paused": stats.paused,
                    "completed": stats.completed,
                    "failed": stats.failed,
                    "uptime_secs": stats.uptime_secs,
                    "pool": { "connects": connects, "reuses": reuses },
                    "global_rate_limit_bps": manager.global_rate_limit(),
                    "refresh_hz": manager.refresh_hz(),
                    "download_dir": manager.download_dir().display().to_string(),
                }))
            }
            "get_settings" => Ok(json!({
                "download_dir": manager.download_dir().display().to_string(),
                "global_rate_limit_bps": manager.global_rate_limit(),
                "refresh_hz": manager.refresh_hz(),
            })),
            "set_settings" => {
                let key = params.get("key").and_then(|v| v.as_str()).unwrap_or("");
                match key {
                    "global_rate_limit_bps" => {
                        let v = params.get("value").and_then(|v| v.as_u64()).unwrap_or(0);
                        manager.set_global_rate_limit(v);
                        Ok(json!({ "key": key, "value": v }))
                    }
                    "refresh_hz" => {
                        let v = params.get("value").and_then(|v| v.as_u64()).unwrap_or(12) as u32;
                        manager.set_refresh_hz(v);
                        Ok(json!({ "key": key, "value": manager.refresh_hz() }))
                    }
                    other => Err(RpcError::new(
                        vdm_rpc::codes::INVALID_PARAMS,
                        format!("unknown or read-only setting: {}", other),
                    )),
                }
            }
            "shutdown" => {
                self.shutdown_flag
                    .store(true, std::sync::atomic::Ordering::Relaxed);
                Ok(json!({ "ok": true }))
            }
            other => Err(RpcError::new(
                vdm_rpc::codes::METHOD_NOT_FOUND,
                format!("unknown method: {}", other),
            )),
        }
    }
}

fn parse_id(params: &Value) -> Result<u64, RpcError> {
    params
        .get("id")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| RpcError::new(vdm_rpc::codes::INVALID_PARAMS, "id must be a non-negative integer"))
}

/// `Manager::add` is async but the RPC handler contract is sync and
/// non-blocking; queueing a task only spawns, so the tiny block_in_place
/// bridge never waits on network IO.
fn futures_block_on_add(manager: &std::sync::Arc<Manager>, spec: vdm_core::TaskSpec) -> Result<u64, RpcError> {
    let manager = manager.clone();
    let result: Result<u64, VdmError> = tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(async move { manager.add(spec).await })
    });
    result.map_err(daemon_err)
}

/// Blocking bridge for the sync RPC handler contract; only used for calls
/// that never wait on network IO (queueing, state flips).
fn block_on<F: std::future::Future>(fut: F) -> F::Output {
    tokio::task::block_in_place(|| tokio::runtime::Handle::current().block_on(fut))
}
