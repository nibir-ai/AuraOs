import QtQuick 2.0;
import calamares.slideshow 1.0;

Presentation
{
    id: presentation

    Timer {
        interval: 15000; running: true; repeat: true
        onTriggered: presentation.nextSlide()
    }

    Slide {
        Text {
            anchors.centerIn: parent
            text: "Welcome to AuraOS"
            font.pixelSize: 28
            color: "#e3e3e3"
            font.bold: true
        }
    }

    Slide {
        Text {
            anchors.centerIn: parent
            text: "Gemini AI Built Directly Into Your OS"
            font.pixelSize: 22
            color: "#e3e3e3"
        }
    }

    Slide {
        Text {
            anchors.centerIn: parent
            text: "Seamless Google Workspace Integration"
            font.pixelSize: 22
            color: "#e3e3e3"
        }
    }
}
