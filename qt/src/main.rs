//! VelocityDM — Qt Quick frontend (S12-lin3 preview).
//!
//! Bootstrap mirrors the gtk3/slint shape: the shared Rust core
//! (`vdm-core`) runs in-process and the RPC server binds the fixed
//! product address `127.0.0.1:36990` (loopback trust boundary inherited
//! from `vdm-rpc`; never reimplemented here). The QML surface is a pure
//! QtQuick (primitives-only, no QtQuick.Controls) Material You chrome
//! driven by the `AppModel` list model in `app_model.rs`.

mod app_model;
mod config;
mod model;
mod tokens;

use cxx_qt_lib::{QGuiApplication, QQmlApplicationEngine, QUrl};
use std::sync::{Arc, OnceLock};

/// The in-process core handed to the QML-instantiated `AppModel`.
/// Created in `main()` BEFORE the QML engine loads, so the model's
/// `initialize` can borrow it; dropped when `main` returns (after
/// `server.stop()` + `engine.shutdown()`).
pub static CORE: OnceLock<Core> = OnceLock::new();

/// Everything the model needs from the bootstrap.
pub struct Core {
    pub engine: Arc<vdm_core::engine::Engine>,
    pub server: Arc<vdm_rpc::RpcServer>,
    pub portable: bool,
}

fn main() {
    let portable = std::env::args().any(|a| a == "--portable");
    let cfg = config::load_config(portable);
    let engine = vdm_core::engine::Engine::new(cfg);
    // fixed-port bind, no fallback: daemon/NMH discovery is fixed-port
    // per docs/RPC.md; never 0.0.0.0
    let server = match vdm_rpc::RpcServer::start(engine.clone(), "127.0.0.1:36990") {
        Ok(s) => s,
        Err(e) => {
            eprintln!("cannot bind 127.0.0.1:36990 ({e}); is another VelocityDM frontend running?");
            std::process::exit(1);
        }
    };
    if CORE
        .set(Core {
            engine: engine.clone(),
            server,
            portable,
        })
        .is_err()
    {
        eprintln!("core already initialized");
        std::process::exit(1);
    }

    let mut app = QGuiApplication::new();
    let mut qml = QQmlApplicationEngine::new();
    if let Some(qml) = qml.as_mut() {
        qml.load(&QUrl::from("qrc:/qt/qml/io/velocitydm/qt/qml/main.qml"));
    }
    if let Some(app) = app.as_mut() {
        app.exec();
    }

    // Lifecycle (spec order): the QML poll timer dies with the QML engine
    // (the last window closed ended `exec`), then the RPC server stops,
    // then the engine shuts down — no orphan process.
    if let Some(core) = CORE.get() {
        core.server.stop();
        core.engine.shutdown();
    }
}
