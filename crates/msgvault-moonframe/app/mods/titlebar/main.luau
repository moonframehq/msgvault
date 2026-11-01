-- The title bar: room for the traffic lights, the brand, a search field in
-- the middle (msgvault's Gmail-like syntax), then where the archive is read
-- from, refresh and the light/dark switch.

type State = { query: string, focus: boolean }

return mod {
	title = "Title bar",

	styles = {
		[".tb"] = { background = "chrome", border = "bottom", border_color = "border", padding = { 0, 12, 0, 0 }, gap = 16 },
		[".tb-logo"] = { background = "accent", color = "accent-foreground", radius = 6 },
		[".tb-name"] = { weight = "bold" },
		[".tb-search"] = { background = "background", border = true, border_color = "border", radius = 6, padding = { 0, 8 }, gap = 6 },
		[".tb-search-icon"] = { color = "muted-foreground" },
		[".tb-field"] = { background = "#00000000", border = "none", shadow = "none", padding = { 3, 0 } },
		[".tb-clear"] = { color = "muted-foreground", radius = 4 },
		[".tb-clear:hover"] = { color = "foreground" },
		[".tb-source"] = { border = true, border_color = "border", radius = 999, padding = { 2, 8 }, gap = 6, color = "muted-foreground", size = "small" },
		[".tb-button"] = { radius = 6, color = "foreground" },
		[".tb-button:hover"] = { background = "list-hover" },
	},

	init = function(previous: State?): State
		return { query = previous and previous.query or "", focus = false }
	end,

	update = function(state: State, msg)
		if msg == "focus" then
			state.focus = true
		elseif type(msg) == "table" and msg.msg == "query" then
			-- Search as the user types, as msgvault-gpui does after a pause.
			state.query = msg.value
			state.focus = false
			return state, { { to = "provider:vault", msg = { query = msg.value } } }
		elseif type(msg) == "table" and msg.msg == "clear" or msg == "clear" then
			state.query = ""
			state.focus = false
			return state, { { to = "provider:vault", msg = { query = "" } } }
		end
	end,

	view = function(state: State, app)
		local vault = app.vault
		local window = app.window
		local inset = if window.platform == "macos" and window.titlebar == "hidden" and not window.fullscreen then 80 else 12
		local source = vault and vault.available and ui.row {
			class = "tb-source",
			align = "center",
			tooltip = if vault.sample then "No msgvault archive was found: showing sample data" else vault.location,
			ui.icon { name = "database", size = "small" },
			ui.text { vault.backend, weight = "medium" },
			ui.text { vault.location, truncate = true },
		}
		return ui.row {
			class = "tb",
			drag = true,
			grow = true,
			height = 34,
			align = "center",
			ui.row {
				gap = "normal",
				align = "center",
				shrink = false,
				ui.row { width = inset - 12, shrink = false },
				ui.row {
					class = "tb-logo",
					width = 22,
					height = 22,
					align = "center",
					justify = "center",
					ui.icon { name = "mails", size = "small" },
				},
				ui.text { "msgvault", class = "tb-name" },
			},
			ui.row {
				grow = true,
				justify = "center",
				ui.row {
					class = "tb-search",
					width = 460,
					height = 24,
					align = "center",
					ui.icon { name = "search", size = "small", class = "tb-search-icon" },
					ui.input {
						id = "search",
						value = state.query,
						placeholder = "Search mail — from:, subject:, label:, has:attachment",
						change = "query",
						keys = { escape = "clear" },
						focus = state.focus,
						grow = true,
						class = "tb-field",
					},
					state.query ~= "" and ui.row {
						class = "tb-clear",
						msg = "clear",
						tooltip = "Clear search",
						ui.icon { name = "circle-x", size = "small" },
					},
				},
			},
			ui.row {
				gap = "normal",
				align = "center",
				shrink = false,
				source,
				ui.row {
					class = "tb-button",
					width = 24,
					height = 24,
					align = "center",
					justify = "center",
					msg = { refresh = true },
					to = "provider:vault",
					tooltip = "Refresh (⌘R)",
					ui.icon { name = "refresh-cw", size = "small" },
				},
				ui.row {
					class = "tb-button",
					width = 24,
					height = 24,
					align = "center",
					justify = "center",
					command = "toggle-theme",
					tooltip = "Toggle dark mode (⌘⇧L)",
					ui.icon { name = if app.dark then "sun" else "moon", size = "small" },
				},
			},
		}
	end,
}
