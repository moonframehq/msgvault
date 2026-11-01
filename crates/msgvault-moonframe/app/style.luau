-- msgvault's look: gpui-kit's neutral theme (white and near-black), with
-- msgvault-gpui's blue accent and 8px corners.
return style {
	-- gpui-kit's text_sm, which most of msgvault-gpui's text is: so `small` is
	-- its text_xs, `large` its 14px names and `title` its headings.
	font_size = 12.25,
	radius = 8,

	colors = {
		accent = { light = "#2563eb", dark = "#3b82f6" },
		["accent-foreground"] = "#ffffff",
		background = { light = "#ffffff", dark = "#0a0a0a" },
		foreground = { light = "#0a0a0a", dark = "#fafafa" },
		sidebar = { light = "#fafafa", dark = "#0a0a0a" },
		card = { light = "#ffffff", dark = "#0a0a0a" },
		muted = { light = "#f5f5f5", dark = "#262626" },
		["muted-foreground"] = { light = "#737373", dark = "#a3a3a3" },
		border = { light = "#e5e5e5", dark = "#262626" },
		warning = { light = "#d97706", dark = "#f59e0b" },
		["list-hover"] = { light = "#f5f5f5", dark = "#171717" },
		["list-active"] = { light = "#f3f3f3", dark = "#262626" },
		-- The title bar, a shade off the window in dark mode.
		chrome = { light = "#ffffff", dark = "#1b1b1b" },
		-- A selected sidebar row.
		["side-active"] = { light = "#ebebeb", dark = "#262626" },
		-- The unread dot and the selected row's bar.
		link = { light = "#2563eb", dark = "#60a5fa" },
		-- Avatars: twelve hues at one lightness (OkLCH), as gpui-kit's
		-- Avatar draws a name: a tint, its letters, and a ring.
		["av0-bg"] = { light = "#ffedf4", dark = "#42232d" },
		["av0-fg"] = { light = "#a13760", dark = "#ffa6c1" },
		["av0-ring"] = { light = "#f8ced9", dark = "#572f3c" },
		["av1-bg"] = { light = "#ffeee9", dark = "#44241f" },
		["av1-fg"] = { light = "#a63a2d", dark = "#ffa99a" },
		["av1-ring"] = { light = "#facfc7", dark = "#59302a" },
		["av2-bg"] = { light = "#fff1e0", dark = "#402712" },
		["av2-fg"] = { light = "#9c4900", dark = "#f9b379" },
		["av2-ring"] = { light = "#f4d4ba", dark = "#54351a" },
		["av3-bg"] = { light = "#fdf5dd", dark = "#382c0c" },
		["av3-fg"] = { light = "#835c00", dark = "#e0c16d" },
		["av3-ring"] = { light = "#e7dab6", dark = "#493b13" },
		["av4-bg"] = { light = "#f2f9e1", dark = "#2b3113" },
		["av4-fg"] = { light = "#5a6e00", dark = "#bbcf7c" },
		["av4-ring"] = { light = "#d6e0bc", dark = "#39421b" },
		["av5-bg"] = { light = "#e7fcea", dark = "#1a3520" },
		["av5-fg"] = { light = "#007932", dark = "#8fd89e" },
		["av5-ring"] = { light = "#c4e5ca", dark = "#24462c" },
		["av6-bg"] = { light = "#dffdf6", dark = "#07362f" },
		["av6-fg"] = { light = "#007c66", dark = "#68dcc7" },
		["av6-ring"] = { light = "#b8e6dc", dark = "#0c473e" },
		["av7-bg"] = { light = "#defcff", dark = "#03343c" },
		["av7-fg"] = { light = "#007790", dark = "#60d8eb" },
		["av7-ring"] = { light = "#b5e4ed", dark = "#05454e" },
		["av8-bg"] = { light = "#e3f9ff", dark = "#133144" },
		["av8-fg"] = { light = "#006aac", dark = "#7eceff" },
		["av8-ring"] = { light = "#bee0f9", dark = "#1c4159" },
		["av9-bg"] = { light = "#ecf5ff", dark = "#242c47" },
		["av9-fg"] = { light = "#445ab5", dark = "#a8c1ff" },
		["av9-ring"] = { light = "#cedafd", dark = "#313b5d" },
		["av10-bg"] = { light = "#f8f1ff", dark = "#322843" },
		["av10-fg"] = { light = "#714ba8", dark = "#cfb4ff" },
		["av10-ring"] = { light = "#dfd4f8", dark = "#423658" },
		["av11-bg"] = { light = "#ffeeff", dark = "#3c243a" },
		["av11-fg"] = { light = "#8f3e8a", dark = "#edaae6" },
		["av11-ring"] = { light = "#efcfeb", dark = "#4f314c" },
	},

	rules = {
		pane = { transition = "slide-fade normal ease-out" },
		[".side-pane"] = { background = "sidebar", border = "right", border_color = "border" },
		[".list-pane"] = { background = "background", border = "right", border_color = "border" },
		[".reader-pane"] = { background = "background" },

		-- Avatars, one class per hue, with the letters' colour.
		[".av"] = { radius = 999, border = true },
		[".av-0"] = { background = "av0-bg", color = "av0-fg", border_color = "av0-ring" },
		[".av-1"] = { background = "av1-bg", color = "av1-fg", border_color = "av1-ring" },
		[".av-2"] = { background = "av2-bg", color = "av2-fg", border_color = "av2-ring" },
		[".av-3"] = { background = "av3-bg", color = "av3-fg", border_color = "av3-ring" },
		[".av-4"] = { background = "av4-bg", color = "av4-fg", border_color = "av4-ring" },
		[".av-5"] = { background = "av5-bg", color = "av5-fg", border_color = "av5-ring" },
		[".av-6"] = { background = "av6-bg", color = "av6-fg", border_color = "av6-ring" },
		[".av-7"] = { background = "av7-bg", color = "av7-fg", border_color = "av7-ring" },
		[".av-8"] = { background = "av8-bg", color = "av8-fg", border_color = "av8-ring" },
		[".av-9"] = { background = "av9-bg", color = "av9-fg", border_color = "av9-ring" },
		[".av-10"] = { background = "av10-bg", color = "av10-fg", border_color = "av10-ring" },
		[".av-11"] = { background = "av11-bg", color = "av11-fg", border_color = "av11-ring" },

		-- Labels as tinted chips (their colour comes inline, from the label).
		[".chip"] = { radius = 4, padding = { 0, 6 } },
		[".chip-round"] = { radius = 999, padding = { 2, 8 }, gap = 4 },
	},
}
