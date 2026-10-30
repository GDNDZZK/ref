pragma Singleton
import QtQuick
import Qutie

// Colors, type and layout metrics. Follows the system color scheme until toggle()
// is called; from then on the choice is the user's.
QtObject {
	property bool dark: Application.styleHints.colorScheme === Qt.Dark

	function toggle() {
		dark = !dark
	}

	readonly property color background: dark ? "#16171a" : "#fafaf9"
	readonly property color surface: dark ? "#1f2024" : "#f1f1ef"
	readonly property color surfaceHover: dark ? "#292a2f" : "#e7e7e4"
	readonly property color border: dark ? "#2d2f35" : "#c8c8c4"
	readonly property color text: dark ? "#e6e6e9" : "#1d1d1f"
	readonly property color textMuted: dark ? "#8e909a" : "#6b6d75"
	readonly property color accent: dark ? "#7aa2f7" : "#94caef"
	readonly property color onAccent: dark ? "#10131a" : "#ffffff"
	readonly property color success: dark ? "#6cc28a" : "#2e9d5b"
	readonly property color danger: dark ? "#ec6f73" : "#d23f45"
	readonly property color selection: dark ? "#2f4a7a" : "#c9dafc"
	readonly property color userBubble: dark ? "#222a3a" : "#e9effc"
	readonly property color userBubbleBorder: dark ? "#2e3a52" : "#d5e1f8"
	readonly property color codeBackground: dark ? "#1b1c20" : "#f4f4f2"
	readonly property color errorSurface: dark ? "#2a1d20" : "#fcefef"
	readonly property color errorBorder: dark ? "#5c2f35" : "#f0c3c5"
	readonly property color scrollHandle: dark ? "#4a4d56" : "#c6c6c2"
	readonly property color scrollHandlePressed: dark ? "#6b6e79" : "#9fa1a8"
	readonly property string syntaxTheme: dark ? "Breeze Dark" : "Breeze Light" // KSyntaxHighlighting theme names

	readonly property string monoFamily: SystemFonts.fixed.family
	readonly property int fontSize: 15
	readonly property int smallFontSize: 12
	readonly property int codeFontSize: 13

	readonly property int radiusSmall: 6
	readonly property int radius: 10
	readonly property int radiusLarge: 18

	readonly property int pagePadding: 24
	readonly property int blockSpacing: 14
	readonly property int turnGap: 18 // extra space around the divider that opens a turn
	readonly property int contentWidth: 760 // reading column
}
