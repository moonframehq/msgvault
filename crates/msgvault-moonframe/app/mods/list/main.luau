-- The conversation list: the mailbox's title and size, then one row per
-- thread (its newest message): unread dot, avatar, sender and date, subject
-- with the thread's size, attachment and star, then the snippet and up to
-- two label chips. Chats lead with the conversation's name.
--
-- In the attachments browser it lists every file instead: a search, filters
-- by kind, then a row per file with its thumbnail or icon, the sender and
-- the message.

local avatar = use "vault-avatar"
local chips_bar = use "chips"

local ROW = 92
local FILE_ROW = 60

return mod {
	region = "list",
	title = "Conversations",

	styles = {
		[".ls-head"] = { border = "bottom", border_color = "border", padding = { 0, 16 } },
		[".ls-title"] = { size = "large", weight = "bold" },
		[".ls-sub"] = { size = "small", color = "muted-foreground" },
		[".ls-row"] = { padding = { 10, 12, 10, 0 }, gap = 12, border = "bottom", border_color = "border/60" },
		[".ls-row:hover"] = { background = "list-hover" },
		[".ls-row:selected"] = { background = "list-active" },
		[".ls-bar"] = { background = "#00000000" },
		[".ls-bar:selected"] = { background = "accent" },
		[".ls-dot"] = { radius = 999 },
		[".ls-dot:selected"] = { background = "accent" },
		[".ls-date"] = { size = "small", color = "muted-foreground" },
		[".ls-date:unread"] = { color = "accent" },
		[".ls-lines"] = { gap = 1 },
		[".ls-muted"] = { color = "muted-foreground" },
		[".ls-count"] = { size = "small", color = "muted-foreground", border = true, border_color = "border", radius = 4, padding = { 0, 4 } },
		[".ls-star"] = { color = "warning" },
		[".ls-empty-tile"] = { background = "muted", radius = 999, color = "muted-foreground" },
		[".fl-tools"] = { padding = { 10, 12, 8, 12 }, gap = 8, border = "bottom", border_color = "border" },
		[".fl-search"] = { background = "background", border = true, border_color = "border", radius = 6, padding = { 0, 8 }, gap = 6, color = "muted-foreground" },
		[".fl-field"] = { background = "#00000000", border = "none", shadow = "none", padding = { 3, 0 } },
		[".fl-row"] = { padding = { 0, 12 }, gap = 12, border = "bottom", border_color = "border/60" },
		[".fl-row:hover"] = { background = "list-hover" },
		[".fl-row:selected"] = { background = "list-active" },
		[".fl-tile"] = { background = "muted", radius = 8, color = "accent" },
		[".fl-thumb"] = { radius = 8 },
		[".fl-name"] = { weight = "medium" },
	},

	view = function(_, app)
		local vault = app.vault
		if not vault then
			return ui.column { grow = true }
		end
		if not vault.available then
			return ui.column {
				grow = true,
				align = "center",
				justify = "center",
				ui.icon { name = "circle-alert", tone = "muted" },
				ui.text { "Unable to load messages", weight = "medium" },
				ui.text { vault.reason or "", tone = "muted", size = "small" },
			}
		end
		local files = vault.mailbox == "files" and vault.files
		if files then
			local file_rows = files.rows
			local kinds = {}
			for _, kind in files.kinds do
				table.insert(kinds, { id = kind.id, label = kind.title, count = kind.count })
			end
			return ui.column {
				grow = true,
				gap = "none",
				ui.column {
					class = "ls-head",
					height = 64,
					justify = "center",
					gap = "none",
					shrink = false,
					ui.text { files.title, class = "ls-title", truncate = true },
					ui.text { files.subtitle, class = "ls-sub", truncate = true },
				},
				ui.column {
					class = "fl-tools",
					shrink = false,
					ui.row {
						class = "fl-search",
						height = 26,
						align = "center",
						ui.icon { name = "search", size = "small" },
						ui.input {
							id = "files-search",
							value = files.query,
							placeholder = "Search by name, sender or subject",
							change = { fquery = true },
							to = "provider:vault",
							keys = { escape = { fquery_clear = true } },
							grow = true,
							class = "fl-field",
						},
					},
					chips_bar { chips = kinds, selected = { files.kind }, event = "kind", to = "provider:vault" },
				},
				if #file_rows == 0
					then ui.column {
						grow = true,
						align = "center",
						justify = "center",
						ui.text { "No attachments match", weight = "bold" },
						ui.text { "Try another kind or other words.", tone = "muted", size = "small" },
					}
					else ui.list {
						id = "files",
						count = #file_rows,
						row_height = FILE_ROW,
						row = function(i)
							local f = file_rows[i]
							return ui.row {
								class = "fl-row",
								height = FILE_ROW,
								align = "center",
								selected = f.id == files.selected,
								msg = { file = f.id },
								to = "provider:vault",
								if f.thumb
									then ui.image { src = f.thumb, width = 40, height = 40, fit = "cover", class = "fl-thumb" }
									else ui.row {
										class = "fl-tile",
										width = 40,
										height = 40,
										align = "center",
										justify = "center",
										shrink = false,
										ui.icon { name = f.icon :: any },
									},
								ui.column {
									grow = true,
									gap = "none",
									ui.row {
										gap = "normal",
										align = "center",
										ui.row { grow = true, ui.text { f.name, class = "fl-name", truncate = true } },
										ui.text { f.date, class = "ls-date" },
									},
									ui.row {
										gap = "normal",
										align = "center",
										ui.row { grow = true, ui.text { `{f.from} · {f.subject}`, class = "ls-muted", truncate = true } },
										ui.text { f.size, class = "ls-date" },
									},
								},
							}
						end,
					},
			}
		end

		local list = vault.list
		local rows = list.rows
		local selected = vault.selected

		local header = ui.column {
			class = "ls-head",
			height = 64,
			justify = "center",
			gap = "none",
			shrink = false,
			ui.text { list.title, class = "ls-title", truncate = true },
			ui.text { list.subtitle, class = "ls-sub", truncate = true },
		}

		if #rows == 0 then
			local searching = vault.query ~= ""
			return ui.column {
				grow = true,
				gap = "none",
				header,
				ui.column {
					grow = true,
					align = "center",
					justify = "center",
					gap = "normal",
					ui.row {
						class = "ls-empty-tile",
						width = 56,
						height = 56,
						align = "center",
						justify = "center",
						ui.icon { name = "inbox", size = "large" },
					},
					ui.text { if searching then "No results" elseif vault.mailbox == "inbox" then "Inbox zero" else "Nothing here", weight = "bold" },
					ui.text {
						if searching then "Try different words, or operators like from: and has:attachment." else "No archived messages in this view.",
						tone = "muted",
						size = "small",
					},
				},
			}
		end

		return ui.column {
			grow = true,
			gap = "none",
			header,
			ui.list {
				id = "rows",
				count = #rows,
				row_height = ROW,
				row = function(i)
					local m = rows[i]
					local on = m.id == selected
					local chips = {}
					for _, chip in m.chips do
						table.insert(chips, ui.row {
							class = "chip",
							shrink = false,
							style = { background = chip.color .. "24" },
							ui.text { chip.title, size = "small", style = { color = chip.color } },
						})
					end
					return ui.row {
						class = "ls-row",
						height = ROW,
						selected = on,
						msg = { open = m.id },
						to = "provider:vault",
						ui.row { class = "ls-bar", width = 3, selected = on },
						ui.row {
							gap = "small",
							shrink = false,
							ui.column {
								height = 34,
								justify = "center",
								ui.row { class = "ls-dot", width = 7, height = 7, selected = m.unread },
							},
							avatar { initials = m.initials, hue = m.hue, size = 34 },
						},
						ui.column {
							class = "ls-lines",
							grow = true,
							ui.row {
								align = "center",
								gap = "normal",
								ui.row {
									grow = true,
									ui.text { m.title, size = "large", weight = if m.unread then "bold" else "medium", truncate = true },
								},
								ui.text { m.date, class = "ls-date", state = if m.unread then "unread" else nil },
							},
							ui.row {
								align = "center",
								gap = "small",
								ui.row {
									grow = true,
									ui.text { m.subject, weight = if m.unread then "bold" else "normal", truncate = true },
								},
								m.count > 0 and ui.row { class = "ls-count", shrink = false, ui.text { tostring(m.count) } },
								m.attachments and ui.icon { name = "paperclip", size = "small", class = "ls-muted" },
								m.starred and ui.icon { name = "star", size = "small", class = "ls-star" },
								m.removed and ui.icon { name = "cloud-off", size = "small", class = "ls-muted" },
							},
							ui.row {
								align = "center",
								gap = "small",
								ui.row {
									grow = true,
									ui.text { m.snippet, class = "ls-muted", truncate = true },
								},
								table.unpack(chips),
							},
						},
					}
				end,
			},
		}
	end,
}
