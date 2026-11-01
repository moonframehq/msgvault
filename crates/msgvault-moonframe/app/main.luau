-- msgvault's shell, after msgvault-gpui: a title bar across the top (the
-- brand, the search field, where the archive is, refresh and the theme),
-- then three panes the user resizes: the sidebar, the conversation list and
-- the reader.

local MENUS: { { title: string, items: { any } } } = {
	{ title = "Mail", items = {
		{ label = "Inbox", msg = { mailbox = "inbox" }, to = "provider:vault", shortcut = "secondary-1" },
		{ label = "Starred", msg = { mailbox = "starred" }, to = "provider:vault", shortcut = "secondary-2" },
		{ label = "Sent", msg = { mailbox = "sent" }, to = "provider:vault", shortcut = "secondary-3" },
		{ label = "All Mail", msg = { mailbox = "all" }, to = "provider:vault", shortcut = "secondary-4" },
		{ label = "All Attachments", msg = { mailbox = "files" }, to = "provider:vault", shortcut = "secondary-5" },
		"-",
		{ label = "Search…", msg = "focus", to = "titlebar", shortcut = "secondary-f" },
		{ label = "Refresh", msg = { refresh = true }, to = "provider:vault", shortcut = "secondary-r" },
	} },
	{ title = "View", items = {
		{ label = "Expand or Collapse the Thread", msg = "expand-all", to = "reader", shortcut = "secondary-;" },
		{ label = "Formatted or Plain Text", msg = "toggle-rich", to = "reader", shortcut = "secondary-shift-h" },
		{ label = "Toggle Dark Mode", command = "toggle-theme", shortcut = "secondary-shift-l" },
		"-",
		{ label = "Show or Hide the Sidebar", msg = "toggle-left", shortcut = "secondary-s" },
	} },
	{ title = "Window", items = {
		{ label = "Minimize", command = "minimize", shortcut = "secondary-m" },
		{ label = "Zoom", command = "maximize" },
	} },
}

return mod {
	window = { titlebar = "hidden", traffic_lights = { x = 12, y = 11 }, size = { width = 1440, height = 900 } },

	about = {
		version = "0.1",
		description = "A reader for a msgvault archive: every account, label, thread and chat it keeps. Shows sample data when no archive is found.",
		website = "https://github.com/penso/msgvault-gpui",
		copyright = "MIT licensed",
	},

	menus = MENUS,

	init = function(previous)
		return previous or { left = true }
	end,

	update = function(state, msg)
		if msg == "toggle-left" then
			state.left = not state.left
		end
	end,

	view = function(state)
		local body = ui.column {
			grow = true,
			gap = "none",
			ui.row { height = 34, shrink = false, gap = "none", ui.mount "titlebar" },
			ui.hsplit {
				id = "panes",
				divider = false,
				ui.pane { size = 250, min = 200, max = 380, fixed = true, visible = state.left, class = "side-pane", ui.region "left" },
				ui.pane { size = 420, min = 320, max = 720, fixed = true, class = "list-pane", ui.region "list" },
				ui.pane { class = "reader-pane", ui.region "main" },
			},
		}
		-- Send feedback (the built-in mods/feedback): ask an agent to change this app.
		return ui.column { grow = true, gap = "none", body, ui.row { class = "feedback-corner", float = { at = "bottom-right", inset = 20 }, ui.mount "feedback" } }
	end,
}
