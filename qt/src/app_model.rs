//! The cxx-qt bridge for the Qt Quick frontend: the `AppModel`
//! QAbstractListModel (id-keyed roles, per-index `dataChanged`, full
//! reset ONLY on membership change) plus every QML-facing invokable and
//! property. This is the ONLY Qt-touching Rust file in the tree.
//!
//! Rendering discipline (spec `S12-LIN3-QT.md`): every daemon-derived
//! string passes `model::display_text` in Rust AND is rendered by QML
//! `Text`/`TextInput` elements carrying the literal
//! `textFormat: Text.PlainText` (enforced by the CI tripwire); all QML
//! is static and embedded (no `createQmlObject`, no runtime `Loader`,
//! no `eval`). Property mutations happen exclusively on the Qt main
//! thread — the 200 ms QML `Timer` invokes `refresh()` there.

// QML property names are camelCase by Qt convention; the qproperty
// macro maps the Rust field name 1:1 to the C++/QML name.
#![allow(non_snake_case)]

use std::pin::Pin;

use cxx_qt_lib::{QModelIndex, QString, QVariant, QVector};

// no bridge namespace: cxx-qt-lib types and the QAbstractListModel base
// live in the C++ global namespace; a bridge namespace would wrongly
// re-declare them inside it (CI evidence-then-fix round 1).
#[cxx_qt::bridge]
mod qobject {
    unsafe extern "C++Qt" {
        include!(<QtCore/QAbstractListModel>);
        /// Base for the task list model.
        #[qobject]
        type QAbstractListModel;
    }

    unsafe extern "C++" {
        include!("cxx-qt-lib/qhash.h");
        type QHash_i32_QByteArray = cxx_qt_lib::QHash<cxx_qt_lib::QHashPair_i32_QByteArray>;

        include!("cxx-qt-lib/qmodelindex.h");
        type QModelIndex = cxx_qt_lib::QModelIndex;

        include!("cxx-qt-lib/qvariant.h");
        type QVariant = cxx_qt_lib::QVariant;

        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;

        include!("cxx-qt-lib/qvector.h");
        type QVector_i32 = cxx_qt_lib::QVector<i32>;
    }

    #[qenum(AppModel)]
    enum Roles {
        Id,
        Name,
        StateLabel,
        StateKind,
        Progress,
        Stats,
        ErrorText,
        NoticeText,
        CanPause,
        CanResume,
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[base = QAbstractListModel]
        // ---- banner / list chrome ----
        #[qproperty(QString, bannerText)]
        #[qproperty(bool, emptyVisible)]
        #[qproperty(QString, addError)]
        // ---- add path ----
        #[qproperty(QString, addUrl)]
        #[qproperty(QString, addFilename)]
        #[qproperty(QString, addQuality)]
        #[qproperty(QString, addLimit)]
        // ---- settings panel ----
        #[qproperty(QString, setDir)]
        #[qproperty(QString, setConns)]
        #[qproperty(QString, setMaxActive)]
        #[qproperty(QString, setLimit)]
        #[qproperty(QString, setProxy)]
        #[qproperty(QString, setFrom)]
        #[qproperty(QString, setTo)]
        #[qproperty(bool, setShutdown)]
        // ---- theme ----
        #[qproperty(QString, seedName)]
        #[qproperty(bool, dark)]
        #[qproperty(QString, colorPrimary)]
        #[qproperty(QString, colorOnPrimary)]
        #[qproperty(QString, colorPrimaryContainer)]
        #[qproperty(QString, colorOnPrimaryContainer)]
        #[qproperty(QString, colorSecondaryContainer)]
        #[qproperty(QString, colorOnSecondaryContainer)]
        #[qproperty(QString, colorTertiaryContainer)]
        #[qproperty(QString, colorOnTertiaryContainer)]
        #[qproperty(QString, colorError)]
        #[qproperty(QString, colorOnError)]
        #[qproperty(QString, colorSurface)]
        #[qproperty(QString, colorOnSurface)]
        #[qproperty(QString, colorSurfaceContainer)]
        #[qproperty(QString, colorOnSurfaceVariant)]
        #[qproperty(QString, colorOutline)]
        #[qproperty(QString, colorOutlineVariant)]
        type AppModel = super::AppModelRust;

        // ---- QAbstractListModel overrides ----
        #[cxx_override]
        #[rust_name = "row_count"]
        fn rowCount(&self, parent: &QModelIndex) -> i32;

        #[cxx_override]
        fn data(&self, index: &QModelIndex, role: i32) -> QVariant;

        #[cxx_override]
        #[rust_name = "role_names"]
        fn roleNames(&self) -> QHash_i32_QByteArray;

        // ---- invokables (all run on the Qt main thread via QML) ----
        /// 200 ms poll: refresh rows, banner, empty state.
        #[qinvokable]
        fn refresh(self: Pin<&mut AppModel>);

        /// Add from the add-path fields (http(s) URL, magnet:, or a
        /// local .torrent path — the gtk3 dispatch).
        #[qinvokable]
        fn addDownload(self: Pin<&mut AppModel>);

        #[qinvokable]
        fn pause(self: Pin<&mut AppModel>, id: u64);

        #[qinvokable]
        fn resume(self: Pin<&mut AppModel>, id: u64);

        #[qinvokable]
        fn remove(self: Pin<&mut AppModel>, id: u64);

        /// Apply the settings panel (merge semantics: starts from the
        /// live engine config so unexposed fields survive).
        #[qinvokable]
        fn applySettings(self: Pin<&mut AppModel>);

        /// Cycle to the next Material You seed and persist the choice.
        #[qinvokable]
        fn cycleSeed(self: Pin<&mut AppModel>);

        /// Toggle dark mode and persist the choice.
        #[qinvokable]
        fn toggleDark(self: Pin<&mut AppModel>);
    }

    unsafe extern "RustQt" {
        /// Inherited dataChanged signal from the QAbstractListModel base
        /// (the in-place per-index update notification).
        #[inherit]
        #[qsignal]
        #[cxx_name = "dataChanged"]
        fn data_changed(
            self: Pin<&mut AppModel>,
            top_left: &QModelIndex,
            bottom_right: &QModelIndex,
            roles: &QVector_i32,
        );

        /// Inherited index() from the base class.
        #[inherit]
        fn index(self: &AppModel, row: i32, column: i32, parent: &QModelIndex) -> QModelIndex;

        /// # Safety
        ///
        /// Inherited beginResetModel; callers must pair it with
        /// end_reset_model.
        #[inherit]
        #[cxx_name = "beginResetModel"]
        unsafe fn begin_reset_model(self: Pin<&mut AppModel>);

        /// # Safety
        ///
        /// Inherited endResetModel; callers must pair it with
        /// begin_reset_model.
        #[inherit]
        #[cxx_name = "endResetModel"]
        unsafe fn end_reset_model(self: Pin<&mut AppModel>);
    }
}

#[derive(Default)]
pub struct AppModelRust {
    // QML-facing properties (managed by the cxx-qt macro)
    bannerText: QString,
    emptyVisible: bool,
    addError: QString,
    addUrl: QString,
    addFilename: QString,
    addQuality: QString,
    addLimit: QString,
    setDir: QString,
    setConns: QString,
    setMaxActive: QString,
    setLimit: QString,
    setProxy: QString,
    setFrom: QString,
    setTo: QString,
    setShutdown: bool,
    seedName: QString,
    dark: bool,
    colorPrimary: QString,
    colorOnPrimary: QString,
    colorPrimaryContainer: QString,
    colorOnPrimaryContainer: QString,
    colorSecondaryContainer: QString,
    colorOnSecondaryContainer: QString,
    colorTertiaryContainer: QString,
    colorOnTertiaryContainer: QString,
    colorError: QString,
    colorOnError: QString,
    colorSurface: QString,
    colorOnSurface: QString,
    colorSurfaceContainer: QString,
    colorOnSurfaceVariant: QString,
    colorOutline: QString,
    colorOutlineVariant: QString,
    // Rust-side state
    rows: Vec<crate::model::RowData>,
    ids: Vec<u64>,
    seed: u32,
}

use cxx_qt::CxxQtType;
use qobject::*;

impl qobject::AppModel {
    fn row_count(&self, _parent: &QModelIndex) -> i32 {
        self.rust().rows.len() as i32
    }

    fn data(&self, index: &QModelIndex, role: i32) -> QVariant {
        let role = Roles { repr: role };
        let Some(row) = self.rust().rows.get(index.row() as usize) else {
            return QVariant::default();
        };
        match role {
            Roles::Id => QVariant::from(&row.id),
            Roles::Name => QVariant::from(&QString::from(row.name.as_str())),
            Roles::StateLabel => QVariant::from(&QString::from(row.state_label)),
            Roles::StateKind => QVariant::from(&QString::from(row.state_kind)),
            Roles::Progress => QVariant::from(&row.progress_permille),
            Roles::Stats => QVariant::from(&QString::from(row.stats.as_str())),
            Roles::ErrorText => QVariant::from(&QString::from(row.error_text.as_str())),
            Roles::NoticeText => QVariant::from(&QString::from(row.notice_text.as_str())),
            Roles::CanPause => QVariant::from(&row.can_pause),
            Roles::CanResume => QVariant::from(&row.can_resume),
            _ => QVariant::default(),
        }
    }

    fn role_names(&self) -> QHash_i32_QByteArray {
        let mut hash = QHash_i32_QByteArray::default();
        hash.insert(Roles::Id.repr, "id".into());
        hash.insert(Roles::Name.repr, "name".into());
        hash.insert(Roles::StateLabel.repr, "stateLabel".into());
        hash.insert(Roles::StateKind.repr, "stateKind".into());
        hash.insert(Roles::Progress.repr, "progress".into());
        hash.insert(Roles::Stats.repr, "stats".into());
        hash.insert(Roles::ErrorText.repr, "errorText".into());
        hash.insert(Roles::NoticeText.repr, "noticeText".into());
        hash.insert(Roles::CanPause.repr, "canPause".into());
        hash.insert(Roles::CanResume.repr, "canResume".into());
        hash
    }

    fn refresh(mut self: Pin<&mut Self>) {
        let Some(core) = crate::CORE.get() else {
            return;
        };
        let snaps = core.engine.list();
        let new_rows = crate::model::rows_from_snapshots(&snaps);
        let new_ids: Vec<u64> = new_rows.iter().map(|r| r.id).collect();

        let membership_changed = new_ids != self.rust().ids;
        if membership_changed {
            unsafe {
                self.as_mut().begin_reset_model();
                self.as_mut().rust_mut().rows = new_rows;
                self.as_mut().rust_mut().ids = new_ids;
                self.as_mut().end_reset_model();
            }
        } else {
            // in-place keyed updates: per-index dataChanged, never a
            // full-list reset
            let changed = crate::model::changed_indices(&self.rust().rows, &new_rows);
            for i in changed {
                let row = new_rows[i].clone();
                self.as_mut().rust_mut().rows[i] = row;
                let idx = self.index(i, 0, &QModelIndex::default());
                let mut roles = QVector::<i32>::default();
                roles.append(Roles::Id.repr);
                roles.append(Roles::Name.repr);
                roles.append(Roles::StateLabel.repr);
                roles.append(Roles::StateKind.repr);
                roles.append(Roles::Progress.repr);
                roles.append(Roles::Stats.repr);
                roles.append(Roles::ErrorText.repr);
                roles.append(Roles::NoticeText.repr);
                roles.append(Roles::CanPause.repr);
                roles.append(Roles::CanResume.repr);
                self.as_mut().data_changed(&idx, &idx, &roles);
            }
        }

        // header banner + empty state
        let total: u64 = snaps.iter().map(|s| s.speed).sum();
        let banner = if total > 0 {
            format!("{}/s total", vdm_core::util::format_bytes(total))
        } else {
            "idle".to_string()
        };
        let empty = snaps.is_empty();
        self.as_mut().set_bannerText(QString::from(banner.as_str()));
        self.as_mut().set_emptyVisible(empty);
    }

    fn addDownload(mut self: Pin<&mut Self>) {
        let Some(core) = crate::CORE.get() else {
            return;
        };
        let url = String::from(&self.rust().addUrl);
        let filename = String::from(&self.rust().addFilename);
        let quality = {
            let q = String::from(&self.rust().addQuality);
            let q = q.trim();
            if q.is_empty() { "best" } else { q }.to_string()
        };
        let limit = {
            let l = String::from(&self.rust().addLimit);
            vdm_core::util::parse_bytes(l.trim()).unwrap_or(0)
        };
        let url_trim = url.trim().to_string();
        let res: Result<(), String> = if url_trim.is_empty() {
            Err("URL is empty".into())
        } else if url_trim.starts_with("magnet:") {
            vdm_core::bt::Magnet::parse(&url_trim)
                .map_err(|e| e.to_string())
                .and_then(|m| {
                    core.engine
                        .add_torrent(vdm_core::bt::BtSpec::Magnet(m), None, vec![], limit)
                        .map(|_| ())
                        .map_err(|e| e.to_string())
                })
        } else if url_trim.ends_with(".torrent") || std::path::Path::new(&url_trim).exists() {
            std::fs::read(&url_trim)
                .map_err(|e| format!("read: {e}"))
                .and_then(|bytes| {
                    core.engine
                        .add_torrent(
                            vdm_core::bt::BtSpec::TorrentBytes(bytes),
                            None,
                            vec![],
                            limit,
                        )
                        .map(|_| ())
                        .map_err(|e| e.to_string())
                })
        } else {
            let opts = vdm_core::engine::HttpOptions {
                quality,
                rate_limit_bps: limit,
                filename: if filename.trim().is_empty() {
                    None
                } else {
                    Some(filename.trim().to_string())
                },
                ..Default::default()
            };
            core.engine
                .add_download(&url_trim, opts)
                .map(|_| ())
                .map_err(|e| e.to_string())
        };
        match res {
            Ok(()) => {
                self.as_mut().set_addError(QString::from(""));
                self.as_mut().set_addUrl(QString::from(""));
                self.as_mut().set_addFilename(QString::from(""));
                self.as_mut().refresh();
            }
            Err(e) => {
                self.as_mut()
                    .set_addError(QString::from(crate::model::display_text(&e).as_str()));
            }
        }
    }

    fn pause(self: Pin<&mut Self>, id: u64) {
        if let Some(core) = crate::CORE.get() {
            let _ = core.engine.pause(id);
        }
    }

    fn resume(self: Pin<&mut Self>, id: u64) {
        if let Some(core) = crate::CORE.get() {
            let _ = core.engine.resume(id);
        }
    }

    fn remove(self: Pin<&mut Self>, id: u64) {
        if let Some(core) = crate::CORE.get() {
            let _ = core.engine.remove(id, false);
        }
    }

    fn applySettings(mut self: Pin<&mut Self>) {
        let Some(core) = crate::CORE.get() else {
            return;
        };
        let fields = crate::model::SettingsFields {
            download_dir: String::from(&self.rust().setDir),
            connections: String::from(&self.rust().setConns),
            max_active: String::from(&self.rust().setMaxActive),
            global_limit: String::from(&self.rust().setLimit),
            proxy: String::from(&self.rust().setProxy),
            stop_from: String::from(&self.rust().setFrom),
            stop_to: String::from(&self.rust().setTo),
            shutdown_when_done: self.rust().setShutdown,
        };
        let mut cfg = core.engine.config();
        if let Err(e) = crate::model::apply_settings(&mut cfg, &fields) {
            self.as_mut()
                .set_addError(QString::from(crate::model::display_text(&e).as_str()));
            return;
        }
        core.engine.set_config(cfg.clone());
        crate::config::save_config(&cfg, core.portable);
        // re-push normalized values to the panel
        self.as_mut()
            .set_setDir(QString::from(cfg.download_dir.to_string_lossy().as_ref()));
        self.as_mut()
            .set_setConns(QString::from(cfg.default_connections.to_string().as_str()));
        self.as_mut()
            .set_setMaxActive(QString::from(cfg.scheduler.max_active.to_string().as_str()));
        self.as_mut().set_setLimit(QString::from(
            if cfg.global_limit_bps > 0 {
                vdm_core::util::format_bytes(cfg.global_limit_bps)
            } else {
                String::new()
            }
            .as_str(),
        ));
        self.as_mut()
            .set_setProxy(QString::from(crate::model::proxy_display(&cfg).as_str()));
        match &cfg.scheduler.stop_window {
            Some((f, t)) => {
                self.as_mut().set_setFrom(QString::from(f.as_str()));
                self.as_mut().set_setTo(QString::from(t.as_str()));
            }
            None => {
                self.as_mut().set_setFrom(QString::from(""));
                self.as_mut().set_setTo(QString::from(""));
            }
        }
        self.as_mut()
            .set_setShutdown(cfg.scheduler.shutdown_when_done);
        self.as_mut().set_addError(QString::from(""));
    }

    fn apply_theme(self: Pin<&mut Self>, seed: u32, dark: bool) {
        let t = crate::tokens::tokens_for(seed, dark);
        self.as_mut()
            .set_colorPrimary(QString::from(t.primary.as_str()));
        self.as_mut()
            .set_colorOnPrimary(QString::from(t.on_primary.as_str()));
        self.as_mut()
            .set_colorPrimaryContainer(QString::from(t.primary_container.as_str()));
        self.as_mut()
            .set_colorOnPrimaryContainer(QString::from(t.on_primary_container.as_str()));
        self.as_mut()
            .set_colorSecondaryContainer(QString::from(t.secondary_container.as_str()));
        self.as_mut()
            .set_colorOnSecondaryContainer(QString::from(t.on_secondary_container.as_str()));
        self.as_mut()
            .set_colorTertiaryContainer(QString::from(t.tertiary_container.as_str()));
        self.as_mut()
            .set_colorOnTertiaryContainer(QString::from(t.on_tertiary_container.as_str()));
        self.as_mut()
            .set_colorError(QString::from(t.error.as_str()));
        self.as_mut()
            .set_colorOnError(QString::from(t.on_error.as_str()));
        self.as_mut()
            .set_colorSurface(QString::from(t.surface.as_str()));
        self.as_mut()
            .set_colorOnSurface(QString::from(t.on_surface.as_str()));
        self.as_mut()
            .set_colorSurfaceContainer(QString::from(t.surface_container.as_str()));
        self.as_mut()
            .set_colorOnSurfaceVariant(QString::from(t.on_surface_variant.as_str()));
        self.as_mut()
            .set_colorOutline(QString::from(t.outline.as_str()));
        self.as_mut()
            .set_colorOutlineVariant(QString::from(t.outline_variant.as_str()));
        self.as_mut()
            .set_seedName(QString::from(crate::tokens::seed_name(seed)));
        self.as_mut().set_dark(dark);
    }

    fn cycleSeed(mut self: Pin<&mut Self>) {
        let Some(core) = crate::CORE.get() else {
            return;
        };
        let (next, _) = crate::tokens::next_seed(self.rust().seed);
        self.as_mut().rust_mut().seed = next;
        let dark = self.rust().dark;
        self.as_mut().apply_theme(next, dark);
        crate::config::save_prefs(&crate::config::UiPrefs { seed: next, dark }, core.portable);
    }

    fn toggleDark(mut self: Pin<&mut Self>) {
        let Some(core) = crate::CORE.get() else {
            return;
        };
        let dark = !self.rust().dark;
        let seed = self.rust().seed;
        self.as_mut().apply_theme(seed, dark);
        crate::config::save_prefs(&crate::config::UiPrefs { seed, dark }, core.portable);
    }
}

impl cxx_qt::Initialize for qobject::AppModel {
    fn initialize(mut self: Pin<&mut Self>) {
        let (cfg, prefs) = match crate::CORE.get() {
            Some(core) => (
                core.engine.config(),
                crate::config::load_prefs(core.portable),
            ),
            None => (
                vdm_core::engine::EngineConfig::default(),
                crate::config::UiPrefs::default(),
            ),
        };
        // add path defaults (quality pinned to "best")
        self.as_mut().set_addQuality(QString::from("best"));
        // settings panel initial values
        self.as_mut()
            .set_setDir(QString::from(cfg.download_dir.to_string_lossy().as_ref()));
        self.as_mut()
            .set_setConns(QString::from(cfg.default_connections.to_string().as_str()));
        self.as_mut()
            .set_setMaxActive(QString::from(cfg.scheduler.max_active.to_string().as_str()));
        self.as_mut().set_setLimit(QString::from(
            if cfg.global_limit_bps > 0 {
                vdm_core::util::format_bytes(cfg.global_limit_bps)
            } else {
                String::new()
            }
            .as_str(),
        ));
        self.as_mut()
            .set_setProxy(QString::from(crate::model::proxy_display(&cfg).as_str()));
        match &cfg.scheduler.stop_window {
            Some((f, t)) => {
                self.as_mut().set_setFrom(QString::from(f.as_str()));
                self.as_mut().set_setTo(QString::from(t.as_str()));
            }
            None => {
                self.as_mut().set_setFrom(QString::from(""));
                self.as_mut().set_setTo(QString::from(""));
            }
        }
        self.as_mut()
            .set_setShutdown(cfg.scheduler.shutdown_when_done);
        // theme
        self.as_mut().rust_mut().seed = prefs.seed;
        self.as_mut().apply_theme(prefs.seed, prefs.dark);
        // first poll
        self.as_mut().refresh();
    }
}
