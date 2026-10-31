// Adapted from an early version of qutie by LightBlueCube
pragma Singleton
import QtQuick
import RefGui

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
	// Attention that is not an error: permission, delay, retry, an interrupted turn.
	readonly property color warning: dark ? "#e6b450" : "#b7791f"
	readonly property color warningSurface: dark ? "#2b2417" : "#fcf4e2"
	readonly property color warningBorder: dark ? "#5a4723" : "#ecd29a"
	readonly property color scrim: dark ? "#8c000000" : "#40000000" // behind dialogs
	// Messages from other sessions, diff hunks, tool input.
	readonly property color info: dark ? "#6fb7e0" : "#2a7fb5"
	readonly property color diffAdded: dark ? "#6cc28a" : "#1f7a4d"
	readonly property color diffAddedBackground: dark ? "#1d2f27" : "#e3f3e9"
	readonly property color diffRemoved: dark ? "#ec6f73" : "#b3303f"
	readonly property color diffRemovedBackground: dark ? "#33202a" : "#fbe6e8"
	readonly property color matchBackground: dark ? "#3a3420" : "#fff1c2" // grep matches
	readonly property real staleOpacity: 0.55 // messages before the last compaction
	// Composer in shell mode (`!command`): amber, like the TUI's warning color.
	readonly property color shellSurface: dark ? "#2a2419" : "#fbf3e3"
	readonly property color shellBorder: dark ? "#f5a742" : "#d9922b"

	// Agent colors, named as in the TUI's configuration (primary, secondary, ...).
	readonly property color agentOrange: dark ? "#fab283" : "#d4783a"
	readonly property color agentBlue: dark ? "#5c9cf5" : "#2f6fd6"
	readonly property color agentPurple: dark ? "#9d7cd8" : "#7a52c2"
	readonly property color agentRed: dark ? "#e06c75" : "#d23f45"
	readonly property color agentYellow: dark ? "#f5a742" : "#c98312"
	readonly property color agentGreen: dark ? "#7fd88f" : "#2e9d5b"
	readonly property color agentCyan: dark ? "#56b6c2" : "#2a8f9c"
	readonly property var agentPalette: [agentBlue, agentPurple, agentGreen, agentOrange, agentRed] // for agents without a color, by position

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

	// Window shell: sidebar, command palette, dialogs, split view, toasts, banner.
	readonly property int windowMinWidth: 640
	readonly property int windowMinHeight: 420
	readonly property int sidebarWidth: 264
	readonly property int sidebarPadding: 8
	readonly property int sidebarHeaderHeight: 52
	readonly property int sidebarRowHeight: 56
	readonly property int sidebarHistoryRowHeight: 44
	readonly property int sidebarSectionHeight: 28
	readonly property int sidebarFooterHeight: 44
	readonly property int sidebarMinRunningRows: 3
	readonly property int sidebarMinHistoryRows: 3
	readonly property int buttonHeight: 34
	readonly property int fieldHeight: 38
	readonly property int paletteWidth: 560
	readonly property int paletteSearchHeight: 52
	readonly property int paletteRowHeight: 40
	readonly property int paletteMaxRows: 9
	readonly property real paletteTopRatio: 0.14
	readonly property int dialogWidth: 480
	readonly property int dialogWidthWide: 640
	readonly property int settingsWidth: 520
	readonly property int dialogHeaderHeight: 56
	readonly property int dialogFooterHeight: 64
	readonly property real dialogMaxHeightRatio: 0.8
	readonly property int tileMinWidth: 420
	readonly property int tileMinHeight: 300
	readonly property int tileGap: 12
	readonly property int tilePadding: 16
	readonly property int tileHeaderHeight: 36
	readonly property int toastWidth: 320
	readonly property int toastMax: 4
	readonly property int bannerHeight: 40

	// The color of an agent: `spec` is its configured color (#rrggbb, #rgb or a name), `index` its position
	// in the agent list, which picks the palette entry when there is no usable spec.
	function agentColor(spec, index) {
		const named = {
			primary: agentOrange, orange: agentOrange,
			secondary: agentBlue, blue: agentBlue,
			accent: agentPurple, purple: agentPurple,
			error: agentRed, red: agentRed,
			warning: agentYellow, yellow: agentYellow,
			success: agentGreen, green: agentGreen,
			info: agentCyan, cyan: agentCyan
		}
		const key = spec.trim().toLowerCase()
		if (named[key] !== undefined)
			return named[key]
		if (/^#([0-9a-f]{3}|[0-9a-f]{6})$/.test(key))
			return key
		return agentPalette[Math.abs(index) % agentPalette.length]
	}
}
