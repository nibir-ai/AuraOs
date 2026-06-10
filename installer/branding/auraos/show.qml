import QtQuick 2.15
import QtQuick.Controls 2.15
import calamares.slideshow 1.0

Presentation {
    id: presentation
    width: 800
    height: 520

    Timer {
        interval: 8000
        running: true
        repeat: true
        onTriggered: presentation.nextSlide()
    }

    // Shared visual background image
    Image {
        id: bgImage
        source: "wallpaper.jpg"
        anchors.fill: parent
        fillMode: Image.PreserveAspectCrop
        opacity: 0.85

        // Dark ambient overlay to guarantee text legibility
        Rectangle {
            anchors.fill: parent
            color: "#111214"
            opacity: 0.5
        }
    }

    Slide {
        id: welcomeSlide

        Column {
            anchors.centerIn: parent
            spacing: 20
            width: 500

            Text {
                text: "Welcome to AuraOS"
                color: "#ffffff"
                font.family: "Inter, sans-serif"
                font.pixelSize: 32
                font.bold: true
                horizontalAlignment: Text.AlignHCenter
                width: parent.width
            }

            Text {
                text: "A modern, secure Linux operating system designed around your Google Identity and workflow."
                color: "#b2bec3"
                font.family: "Inter, sans-serif"
                font.pixelSize: 16
                lineHeight: 1.4
                wrapMode: Text.WordWrap
                horizontalAlignment: Text.AlignHCenter
                width: parent.width
            }
        }
    }

    Slide {
        id: aiSlide

        Column {
            anchors.centerIn: parent
            spacing: 20
            width: 500

            Text {
                text: "Built-In Gemini Assistant"
                color: "#4b7bec" // Accent blue
                font.family: "Inter, sans-serif"
                font.pixelSize: 28
                font.bold: true
                horizontalAlignment: Text.AlignHCenter
                width: parent.width
            }

            Text {
                text: "Access a localized, agentic AI assistant via a keyboard shortcut (Super+G) or top panel. Gemini handles queries, updates your schedule, drafts emails, and queries files directly from your workspace."
                color: "#e3e3e3"
                font.family: "Inter, sans-serif"
                font.pixelSize: 15
                lineHeight: 1.4
                wrapMode: Text.WordWrap
                horizontalAlignment: Text.AlignHCenter
                width: parent.width
            }
        }
    }

    Slide {
        id: authSlide

        Column {
            anchors.centerIn: parent
            spacing: 20
            width: 500

            Text {
                text: "Google Identity is System Identity"
                color: "#ffffff"
                font.family: "Inter, sans-serif"
                font.pixelSize: 28
                font.bold: true
                horizontalAlignment: Text.AlignHCenter
                width: parent.width
            }

            Text {
                text: "Sign in once with Google to initialize your Linux profile. AuraOS integrates authentication, GNOME keyrings, secure token rotation, and offline fallback PIN validation."
                color: "#b2bec3"
                font.family: "Inter, sans-serif"
                font.pixelSize: 15
                lineHeight: 1.4
                wrapMode: Text.WordWrap
                horizontalAlignment: Text.AlignHCenter
                width: parent.width
            }
        }
    }

    Slide {
        id: syncSlide

        Column {
            anchors.centerIn: parent
            spacing: 20
            width: 500

            Text {
                text: "Lazy Drive Mounting & Profile Sync"
                color: "#4b7bec"
                font.family: "Inter, sans-serif"
                font.pixelSize: 28
                font.bold: true
                horizontalAlignment: Text.AlignHCenter
                width: parent.width
            }

            Text {
                text: "Access your cloud files on-demand in the file browser under ~/Drive, and sync names, avatars, calendar events, and contacts automatically with standard system applications."
                color: "#e3e3e3"
                font.family: "Inter, sans-serif"
                font.pixelSize: 15
                lineHeight: 1.4
                wrapMode: Text.WordWrap
                horizontalAlignment: Text.AlignHCenter
                width: parent.width
            }
        }
    }
}
