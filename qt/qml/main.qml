// VelocityDM — Qt Quick frontend (S12-lin3 preview).
//
// Rendering discipline (spec S12-LIN3-QT.md):
//   * every text-bearing element carries the literal
//     `textFormat: Text.PlainText` — never QML's default AutoText
//     (which sniffs HTML into rich text, including remote <img> fetches);
//     StyledText/RichText/MarkdownText are forbidden tree-wide (CI
//     tripwire);
//   * all QML/JS here is static and embedded — no createQmlObject, no
//     createComponent from runtime strings, no Loader with a runtime
//     source, no eval;
//   * no link handlers on daemon-derived strings;
//   * chrome is built from QtQuick primitives only (no QtQuick.Controls
//     import) — the "MD3 chrome from primitives" approach shared with
//     the Slint tree; colors come from the AppModel token properties
//     (hex strings built in Rust from the shared vdm-md3 scheme).
import QtQuick 2.15
import QtQuick.Window 2.15

import io.velocitydm.qt 1.0

Window {
    id: root
    width: 900
    height: 640
    visible: true
    title: "VelocityDM"

    AppModel {
        id: app
    }

    // 200 ms poll (gtk3 parity; SSE consumption is a recorded deferred
    // upgrade — named in the README and the promotion-gates matrix)
    Timer {
        interval: 200
        running: true
        repeat: true
        onTriggered: app.refresh()
    }

    Rectangle {
        anchors.fill: parent
        color: app.colorSurface

        Column {
            anchors.fill: parent
            anchors.margins: 12
            spacing: 8

            // ---- header ----
            Rectangle {
                width: parent.width
                height: 44
                radius: 20
                color: app.colorSurfaceContainer

                Row {
                    anchors.fill: parent
                    anchors.margins: 8
                    spacing: 8

                    Text {
                        textFormat: Text.PlainText
                        text: "VelocityDM"
                        color: app.colorOnSurface
                        font.pixelSize: 18
                        font.bold: true
                        anchors.verticalCenter: parent.verticalCenter
                    }
                    Text {
                        textFormat: Text.PlainText
                        text: app.bannerText
                        color: app.colorOnSurfaceVariant
                        font.pixelSize: 13
                        anchors.verticalCenter: parent.verticalCenter
                    }
                    Item { width: parent.width - 480; height: 1 }
                    Rectangle {
                        width: 150
                        height: 30
                        radius: 15
                        color: app.colorSecondaryContainer
                        anchors.verticalCenter: parent.verticalCenter
                        Text {
                            textFormat: Text.PlainText
                            anchors.centerIn: parent
                            text: app.seedName
                            color: app.colorOnSecondaryContainer
                            font.pixelSize: 12
                            maximumLineCount: 1
                            elide: Text.ElideRight
                            width: parent.width - 12
                        }
                        MouseArea {
                            anchors.fill: parent
                            onClicked: app.cycleSeed()
                        }
                    }
                    Rectangle {
                        width: 70
                        height: 30
                        radius: 15
                        color: app.colorSecondaryContainer
                        anchors.verticalCenter: parent.verticalCenter
                        Text {
                            textFormat: Text.PlainText
                            anchors.centerIn: parent
                            text: app.dark ? "dark" : "light"
                            color: app.colorOnSecondaryContainer
                            font.pixelSize: 12
                        }
                        MouseArea {
                            anchors.fill: parent
                            onClicked: app.toggleDark()
                        }
                    }
                }
            }

            // ---- add row ----
            Rectangle {
                width: parent.width
                height: 84
                radius: 16
                color: app.colorSurfaceContainer

                Column {
                    anchors.fill: parent
                    anchors.margins: 8
                    spacing: 6

                    Row {
                        spacing: 6
                        width: parent.width

                        Rectangle {
                            width: 430
                            height: 30
                            radius: 8
                            color: app.colorSurface
                            border.color: app.colorOutline
                            border.width: 1
                            TextInput {
                                textFormat: Text.PlainText
                                anchors.fill: parent
                                anchors.margins: 6
                                text: app.addUrl
                                onTextChanged: app.addUrl = text
                                color: app.colorOnSurface
                                font.pixelSize: 13
                                clip: true
                                maximumLength: 4096
                            }
                        }
                        Rectangle {
                            width: 160
                            height: 30
                            radius: 8
                            color: app.colorSurface
                            border.color: app.colorOutline
                            border.width: 1
                            TextInput {
                                textFormat: Text.PlainText
                                anchors.fill: parent
                                anchors.margins: 6
                                text: app.addFilename
                                onTextChanged: app.addFilename = text
                                color: app.colorOnSurface
                                font.pixelSize: 13
                                clip: true
                            }
                        }
                        Rectangle {
                            width: 70
                            height: 30
                            radius: 8
                            color: app.colorSurface
                            border.color: app.colorOutline
                            border.width: 1
                            TextInput {
                                textFormat: Text.PlainText
                                anchors.fill: parent
                                anchors.margins: 6
                                text: app.addQuality
                                onTextChanged: app.addQuality = text
                                color: app.colorOnSurface
                                font.pixelSize: 13
                                clip: true
                            }
                        }
                        Rectangle {
                            width: 90
                            height: 30
                            radius: 8
                            color: app.colorSurface
                            border.color: app.colorOutline
                            border.width: 1
                            TextInput {
                                textFormat: Text.PlainText
                                anchors.fill: parent
                                anchors.margins: 6
                                text: app.addLimit
                                onTextChanged: app.addLimit = text
                                color: app.colorOnSurface
                                font.pixelSize: 13
                                clip: true
                            }
                        }
                        Rectangle {
                            width: 90
                            height: 30
                            radius: 15
                            color: app.colorPrimaryContainer
                            Text {
                                textFormat: Text.PlainText
                                anchors.centerIn: parent
                                text: "Add"
                                color: app.colorOnPrimaryContainer
                                font.pixelSize: 13
                                font.bold: true
                            }
                            MouseArea {
                                anchors.fill: parent
                                onClicked: app.addDownload()
                            }
                        }
                    }
                    Text {
                        textFormat: Text.PlainText
                        text: "URL (https://…, magnet:, or a local .torrent path) · filename override · quality (best|worst|bandwidth) · speed limit"
                        color: app.colorOnSurfaceVariant
                        font.pixelSize: 10
                        width: parent.width
                        maximumLineCount: 1
                        elide: Text.ElideRight
                    }
                    Text {
                        textFormat: Text.PlainText
                        text: app.addError
                        color: app.colorError
                        font.pixelSize: 11
                        visible: app.addError.length > 0
                        width: parent.width
                        maximumLineCount: 2
                        wrapMode: Text.WrapAnywhere
                    }
                }
            }

            // ---- task list ----
            ListView {
                id: list
                width: parent.width
                height: parent.height - 44 - 84 - 190 - 32
                clip: true
                spacing: 8
                model: app

                delegate: Rectangle {
                    width: list.width
                    height: 96
                    radius: 16
                    color: app.colorSurfaceContainer

                    Column {
                        anchors.fill: parent
                        anchors.margins: 10
                        spacing: 4

                        Row {
                            spacing: 8
                            width: parent.width

                            Rectangle {
                                width: 64
                                height: 22
                                radius: 11
                                color: model.stateKind === "active" ? app.colorPrimaryContainer :
                                       model.stateKind === "failed" ? app.colorError :
                                       model.stateKind === "done" ? app.colorTertiaryContainer :
                                       app.colorSecondaryContainer
                                Text {
                                    textFormat: Text.PlainText
                                    anchors.centerIn: parent
                                    text: model.stateLabel
                                    color: model.stateKind === "failed" ? app.colorOnError :
                                           model.stateKind === "done" ? app.colorOnTertiaryContainer :
                                           model.stateKind === "active" ? app.colorOnPrimaryContainer :
                                           app.colorOnSecondaryContainer
                                    font.pixelSize: 11
                                    font.bold: true
                                }
                            }
                            Text {
                                textFormat: Text.PlainText
                                text: model.name
                                color: app.colorOnSurface
                                font.pixelSize: 14
                                font.bold: true
                                width: parent.width - 64 - 230
                                maximumLineCount: 1
                                elide: Text.ElideMiddle
                                anchors.verticalCenter: parent.verticalCenter
                            }
                            Rectangle {
                                width: 70
                                height: 26
                                radius: 13
                                color: app.colorSecondaryContainer
                                visible: model.canPause
                                Text {
                                    textFormat: Text.PlainText
                                    anchors.centerIn: parent
                                    text: "Pause"
                                    color: app.colorOnSecondaryContainer
                                    font.pixelSize: 11
                                }
                                MouseArea {
                                    anchors.fill: parent
                                    onClicked: app.pause(model.id)
                                }
                            }
                            Rectangle {
                                width: 78
                                height: 26
                                radius: 13
                                color: app.colorSecondaryContainer
                                visible: model.canResume
                                Text {
                                    textFormat: Text.PlainText
                                    anchors.centerIn: parent
                                    text: "Resume"
                                    color: app.colorOnSecondaryContainer
                                    font.pixelSize: 11
                                }
                                MouseArea {
                                    anchors.fill: parent
                                    onClicked: app.resume(model.id)
                                }
                            }
                            Rectangle {
                                width: 78
                                height: 26
                                radius: 13
                                color: app.colorSecondaryContainer
                                Text {
                                    textFormat: Text.PlainText
                                    anchors.centerIn: parent
                                    text: "Remove"
                                    color: app.colorOnSecondaryContainer
                                    font.pixelSize: 11
                                }
                                MouseArea {
                                    anchors.fill: parent
                                    onClicked: app.remove(model.id)
                                }
                            }
                        }

                        // progress track + fill
                        Rectangle {
                            width: parent.width
                            height: 8
                            radius: 4
                            color: app.colorOutlineVariant
                            Rectangle {
                                width: Math.max(0, Math.min(1.0, model.progress / 1000.0)) * parent.width
                                height: parent.height
                                radius: 4
                                color: app.colorPrimary
                            }
                        }
                        Text {
                            textFormat: Text.PlainText
                            text: model.stats
                            color: app.colorOnSurfaceVariant
                            font.pixelSize: 11
                            width: parent.width
                            maximumLineCount: 1
                            elide: Text.ElideRight
                        }
                        Text {
                            textFormat: Text.PlainText
                            text: model.errorText
                            color: app.colorError
                            font.pixelSize: 11
                            visible: model.errorText.length > 0
                            width: parent.width
                            maximumLineCount: 1
                            elide: Text.ElideRight
                        }
                        Text {
                            textFormat: Text.PlainText
                            text: model.noticeText
                            color: app.colorOnSurfaceVariant
                            font.pixelSize: 11
                            visible: model.noticeText.length > 0
                            width: parent.width
                            maximumLineCount: 1
                            elide: Text.ElideRight
                        }
                    }
                }

                Text {
                    textFormat: Text.PlainText
                    text: "No downloads yet"
                    color: app.colorOnSurfaceVariant
                    font.pixelSize: 14
                    anchors.centerIn: parent
                    visible: app.emptyVisible
                }
            }

            // ---- settings panel ----
            Rectangle {
                width: parent.width
                height: 190
                radius: 16
                color: app.colorSurfaceContainer

                Column {
                    anchors.fill: parent
                    anchors.margins: 8
                    spacing: 6

                    Row {
                        spacing: 6
                        Rectangle {
                            width: 300
                            height: 28
                            radius: 8
                            color: app.colorSurface
                            border.color: app.colorOutline
                            border.width: 1
                            TextInput {
                                textFormat: Text.PlainText
                                anchors.fill: parent
                                anchors.margins: 6
                                text: app.setDir
                                onTextChanged: app.setDir = text
                                color: app.colorOnSurface
                                font.pixelSize: 12
                                clip: true
                            }
                        }
                        Rectangle {
                            width: 60
                            height: 28
                            radius: 8
                            color: app.colorSurface
                            border.color: app.colorOutline
                            border.width: 1
                            TextInput {
                                textFormat: Text.PlainText
                                anchors.fill: parent
                                anchors.margins: 6
                                text: app.setConns
                                onTextChanged: app.setConns = text
                                color: app.colorOnSurface
                                font.pixelSize: 12
                            }
                        }
                        Rectangle {
                            width: 60
                            height: 28
                            radius: 8
                            color: app.colorSurface
                            border.color: app.colorOutline
                            border.width: 1
                            TextInput {
                                textFormat: Text.PlainText
                                anchors.fill: parent
                                anchors.margins: 6
                                text: app.setMaxActive
                                onTextChanged: app.setMaxActive = text
                                color: app.colorOnSurface
                                font.pixelSize: 12
                            }
                        }
                        Rectangle {
                            width: 90
                            height: 28
                            radius: 8
                            color: app.colorSurface
                            border.color: app.colorOutline
                            border.width: 1
                            TextInput {
                                textFormat: Text.PlainText
                                anchors.fill: parent
                                anchors.margins: 6
                                text: app.setLimit
                                onTextChanged: app.setLimit = text
                                color: app.colorOnSurface
                                font.pixelSize: 12
                            }
                        }
                        Rectangle {
                            width: 180
                            height: 28
                            radius: 8
                            color: app.colorSurface
                            border.color: app.colorOutline
                            border.width: 1
                            TextInput {
                                textFormat: Text.PlainText
                                anchors.fill: parent
                                anchors.margins: 6
                                text: app.setProxy
                                onTextChanged: app.setProxy = text
                                color: app.colorOnSurface
                                font.pixelSize: 12
                                clip: true
                            }
                        }
                    }
                    Row {
                        spacing: 6
                        height: 28
                        Rectangle {
                            width: 70
                            height: 28
                            radius: 8
                            color: app.colorSurface
                            border.color: app.colorOutline
                            border.width: 1
                            TextInput {
                                textFormat: Text.PlainText
                                anchors.fill: parent
                                anchors.margins: 6
                                text: app.setFrom
                                onTextChanged: app.setFrom = text
                                color: app.colorOnSurface
                                font.pixelSize: 12
                            }
                        }
                        Rectangle {
                            width: 70
                            height: 28
                            radius: 8
                            color: app.colorSurface
                            border.color: app.colorOutline
                            border.width: 1
                            TextInput {
                                textFormat: Text.PlainText
                                anchors.fill: parent
                                anchors.margins: 6
                                text: app.setTo
                                onTextChanged: app.setTo = text
                                color: app.colorOnSurface
                                font.pixelSize: 12
                            }
                        }
                        Text {
                            textFormat: Text.PlainText
                            text: "stop window (HH:MM both ends)"
                            color: app.colorOnSurfaceVariant
                            font.pixelSize: 10
                            anchors.verticalCenter: parent.verticalCenter
                        }
                        Rectangle {
                            width: 120
                            height: 28
                            radius: 8
                            color: app.colorSecondaryContainer
                            Text {
                                textFormat: Text.PlainText
                                anchors.centerIn: parent
                                text: app.setShutdown ? "shutdown: on" : "shutdown: off"
                                color: app.colorOnSecondaryContainer
                                font.pixelSize: 11
                            }
                            MouseArea {
                                anchors.fill: parent
                                onClicked: app.setShutdown = !app.setShutdown
                            }
                        }
                        Rectangle {
                            width: 90
                            height: 28
                            radius: 14
                            color: app.colorPrimaryContainer
                            Text {
                                textFormat: Text.PlainText
                                anchors.centerIn: parent
                                text: "Apply"
                                color: app.colorOnPrimaryContainer
                                font.pixelSize: 12
                                font.bold: true
                            }
                            MouseArea {
                                anchors.fill: parent
                                onClicked: app.applySettings()
                            }
                        }
                    }
                    Text {
                        textFormat: Text.PlainText
                        text: "download dir · connections (1-32) · max active (1-16) · global limit · proxy (http://… | socks5://… | none) — Apply keeps unexposed engine settings"
                        color: app.colorOnSurfaceVariant
                        font.pixelSize: 10
                        width: parent.width
                        maximumLineCount: 1
                        elide: Text.ElideRight
                    }
                }
            }
        }
    }
}
