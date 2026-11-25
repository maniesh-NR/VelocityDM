// S12-lin3: the pure-cargo Qt Quick build (the cxx-qt
// "cargo_without_cmake" shape — no CMake, no C++ bootstrap file; qmake is
// probed from QMAKE or the PATH, see the CI job's qmake6 insurance).
use cxx_qt_build::{CxxQtBuilder, QmlModule};

fn main() {
    CxxQtBuilder::new_qml_module(QmlModule::new("io.velocitydm.qt").qml_file("qml/main.qml"))
        .files(["src/app_model.rs"])
        .build();
}
