-- The reader: the thread's subject, its labels and size, then its messages.
-- Email threads read like Gmail's: earlier messages fold to one line (a
-- click opens one), the newest is open, as a card with the sender, the
-- recipients, the body and the attachments. Chats are bubbles: the others'
-- on the left under their names, yours on the right in the accent.
--
-- HTML mail shows rendered, in a web view on a page the provider writes
-- (no scripts, no remote pictures until "Load remote images"), or as plain
-- text with the toolbar's toggle. In the attachments browser it shows the
-- chosen file instead.

local avatar = use "vault-avatar"
local file_card = use "vault-file"

type State = {
	thread: number,
	open: { [string]: boolean? },
	all: boolean,
	rich: boolean,
	away: { [string]: string? },
	seen: number,
}

local function fresh(): State
	return { thread = 0, open = {}, all = false, rich = true, away = {}, seen = 0 }
end

-- The most pages drawn at once: a mod may hold four web views.
local PAGES = 3

return mod {
	region = "main",
	title = "Reader",
	subscribe = { "provider/vault" },

	styles = {
		[".rd-scroll"] = { padding = { 18, 32, 48, 28 } },
		[".rd-body"] = { gap = 14 },
		[".rd-head"] = { gap = 6 },
		[".rd-subject"] = { size = "title", weight = "bold" },
		[".rd-meta"] = { size = "small", color = "muted-foreground" },
		[".rd-tool"] = { radius = 6, color = "foreground" },
		[".rd-tool:hover"] = { background = "list-hover" },
		[".rd-fold"] = { border = true, border_color = "border", radius = 10, padding = { 0, 16 }, gap = 12 },
		[".rd-fold:hover"] = { background = "list-hover" },
		[".rd-card"] = { border = true, border_color = "border", radius = 10, background = "background", shadow = "0 1px 2px #0000000d" },
		[".rd-card-head"] = { padding = 16, gap = 12 },
		[".rd-card-head:hover"] = { background = "list-hover" },
		[".rd-name"] = { size = "large", weight = "bold" },
		[".rd-muted"] = { color = "muted-foreground" },
		[".rd-small"] = { size = "small", color = "muted-foreground" },
		[".rd-text"] = { padding = { 0, 16, 20, 68 } },
		[".rd-files"] = { padding = { 0, 16, 16, 68 }, gap = 10 },
		[".rd-star"] = { color = "warning" },
		[".rd-sender"] = { size = "small", weight = "medium", color = "muted-foreground" },
		[".rd-group"] = { padding = { 12, 0, 2, 0 } },
		[".rd-bubble"] = { radius = 16, padding = { 6, 12 }, background = "muted" },
		[".rd-bubble-mine"] = { background = "accent", color = "accent-foreground" },
		[".rd-empty-tile"] = { background = "muted", radius = 999, color = "muted-foreground" },
		[".rd-tool:selected"] = { background = "muted" },
		[".rd-page"] = { padding = { 0, 16, 16, 68 }, gap = 8 },
		[".rd-web"] = { background = "background", border = "none", padding = 0 },
		[".rd-remote"] = { background = "muted/60", radius = 8, padding = { 6, 6, 6, 10 }, gap = 8, size = "small", color = "muted-foreground" },
		[".rd-remote-load"] = { radius = 6, padding = { 3, 8 }, color = "accent", weight = "medium" },
		[".rd-remote-load:hover"] = { background = "accent/10" },
		[".rd-detail"] = { padding = { 24, 32, 32, 32 }, gap = 16 },
		[".rd-field"] = { gap = 12 },
		[".rd-label"] = { size = "small", color = "muted-foreground" },
		[".rd-action"] = { radius = 8, padding = { 7, 14 }, gap = 8, border = true, border_color = "border", weight = "medium" },
		[".rd-action:hover"] = { background = "list-hover" },
		[".rd-action-primary"] = { background = "accent", color = "accent-foreground", border_color = "accent" },
		[".rd-action-primary:hover"] = { background = "accent/90" },
		[".rd-link"] = { color = "accent" },
		[".rd-toast"] = { padding = { 10, 10, 10, 14 }, gap = 10 },
	},

	init = function(previous: State?): State
		local state = fresh()
		if previous then
			state.thread = previous.thread or 0
			state.open = previous.open or {}
			state.all = previous.all or false
			state.rich = previous.rich ~= false
			state.seen = previous.seen or 0
		end
		return state
	end,

	update = function(state: State, msg)
		if msg == "toggle-rich" then
			state.rich = not state.rich
			return
		end
		if type(msg) ~= "table" and msg ~= "expand-all" then
			return
		end
		if msg == "expand-all" then
			state.all = not state.all
			state.open = {}
		elseif msg.dismiss then
			state.seen = msg.dismiss
		elseif msg.page then
			-- A page left the message (a link was clicked): offer the way back.
			local key = tostring(msg.page)
			local url = msg.url or ""
			state.away[key] = if string.sub(url, 1, 7) ~= "file://" and url ~= "" and url ~= "about:blank" then url else nil
		elseif msg.topic == "provider/vault" then
			local thread = msg.value and msg.value.thread
			local id = thread and thread.id or 0
			if id ~= state.thread then
				state.thread = id
				state.open = {}
				state.all = false
				state.away = {}
			end
		elseif msg.toggle then
			local key = tostring(msg.toggle)
			state.open[key] = not state.open[key] or nil
		end
	end,

	view = function(state: State, app)
		local vault = app.vault
		-- What Open and Save did, in a corner until dismissed.
		local notice = vault and vault.notice
		local toast = notice and notice.seq > state.seen and ui.overlay {
			open = true,
			place = "bottom-end",
			width = 340,
			content = ui.row {
				class = "rd-toast",
				align = "center",
				ui.icon { name = if notice.ok then "circle-check" else "circle-alert", tone = if notice.ok then "success" else "danger" },
				ui.row { grow = true, ui.text { notice.text, wrap = true } },
				ui.row {
					class = "rd-tool",
					width = 24,
					height = 24,
					align = "center",
					justify = "center",
					msg = { dismiss = notice.seq },
					ui.icon { name = "close", size = "small" },
				},
			},
		}

		if vault and vault.available and vault.mailbox == "files" and vault.files then
			local file = vault.files.file
			if not file then
				return ui.column {
					grow = true,
					align = "center",
					justify = "center",
					gap = "normal",
					ui.row {
						class = "rd-empty-tile",
						width = 56,
						height = 56,
						align = "center",
						justify = "center",
						ui.icon { name = "paperclip", size = "large" },
					},
					ui.text { "No attachment selected", weight = "bold" },
					ui.text { "Pick a file to see it, open it or save it.", tone = "muted", size = "small" },
					toast,
				}
			end
			local function field(label: string, value: any)
				return ui.row {
					class = "rd-field",
					align = "start",
					ui.row { width = 90, shrink = false, ui.text { label, class = "rd-label" } },
					ui.row { grow = true, if type(value) == "string" then ui.text { value, wrap = true } else value },
				}
			end
			return ui.column {
				grow = true,
				scroll = true,
				gap = "none",
				ui.row {
					class = "rd-detail",
					justify = "center",
					shrink = false,
					ui.column {
						grow = true,
						max_width = 720,
						gap = "large",
						file_card { file = file, large = true },
						ui.column {
							gap = "small",
							ui.text { file.name, class = "rd-subject" },
							ui.text { `{file.type} · {file.size}`, class = "rd-meta" },
						},
						ui.row {
							gap = "normal",
							ui.row {
								class = { "rd-action", "rd-action-primary" },
								align = "center",
								msg = { open_file = file.id },
								to = "provider:vault",
								ui.icon { name = "external-link", size = "small" },
								ui.text { "Open" },
							},
							ui.row {
								class = "rd-action",
								align = "center",
								msg = { save_file = file.id },
								to = "provider:vault",
								ui.icon { name = "download", size = "small" },
								ui.text { "Save to Downloads" },
							},
						},
						ui.divider(),
						ui.column {
							gap = "normal",
							field("From", file.from),
							field("Message", ui.row {
								msg = { show = file.message },
								to = "provider:vault",
								tooltip = "Show the message",
								ui.text { file.subject, class = "rd-link", wrap = true },
							}),
							field("Received", file.date),
						},
					},
				},
				toast,
			}
		end

		local thread = vault and vault.available and vault.thread
		if not thread then
			return ui.column {
				grow = true,
				align = "center",
				justify = "center",
				gap = "normal",
				ui.row {
					class = "rd-empty-tile",
					width = 56,
					height = 56,
					align = "center",
					justify = "center",
					ui.icon { name = "mail", size = "large" },
				},
				ui.text { "No message selected", weight = "bold" },
				ui.text { "Pick a conversation from the list, or search with / and ⌘F.", tone = "muted", size = "small" },
				toast,
			}
		end

		local messages = thread.messages
		local items: { any } = {}
		if thread.chat then
			local previous: string? = nil
			for _, m in messages do
				local who = if m.mine then "\0you" else m.from
				local first = who ~= previous
				previous = who
				if first then
					table.insert(items, ui.row {
						class = "rd-group",
						justify = if m.mine then "end" else "start",
						ui.row { width = if m.mine then 0 else 28, shrink = false },
						ui.text { if m.mine then "You" else m.from, class = "rd-sender" },
						ui.row { width = if m.mine then 52 else 0, shrink = false },
					})
				end
				local bubble = ui.column {
					class = if m.mine then { "rd-bubble", "rd-bubble-mine" } else "rd-bubble",
					max_width = 520,
					ui.text { m.body },
				}
				local time = ui.text { m.time, class = "rd-small" }
				if m.mine then
					table.insert(items, ui.row { align = "end", justify = "end", gap = "normal", bubble, time })
				else
					table.insert(items, ui.row {
						align = "end",
						gap = "normal",
						if first then avatar { initials = m.initials, hue = m.hue, size = 28 } else ui.row { width = 28, shrink = false },
						bubble,
						time,
					})
				end
			end
		else
			local many = #messages > 1
			local pages = 0
			for index = #messages, 1, -1 do
				local m = messages[index]
				local open = not many or index == #messages or state.all or state.open[tostring(m.id)] == true
				if not open then
					table.insert(items, ui.row {
						class = "rd-fold",
						height = 50,
						align = "center",
						msg = { toggle = m.id },
						avatar { initials = m.initials, hue = m.hue, size = 28 },
						ui.text { m.from, class = "rd-name" },
						ui.row { grow = true, ui.text { m.snippet, class = "rd-muted", truncate = true } },
						m.attachments and ui.icon { name = "paperclip", size = "small", class = "rd-muted" },
						ui.text { m.date, class = "rd-small" },
					})
				else
					local recipients = {}
					for _, part in { { "to", m.to }, { "cc", m.cc } } do
						if part[2] ~= "" then
							table.insert(recipients, ui.row {
								gap = "small",
								ui.text { part[1], class = "rd-small", weight = "medium" },
								ui.text { part[2], class = "rd-small", truncate = true },
							})
						end
					end
					local files = {}
					for _, file in m.files do
						table.insert(files, file_card { file = file })
					end
					local html = m.html
					local body
					if html and state.rich and pages < PAGES then
						pages += 1
						local id = `page-{m.id}`
						local away = state.away[tostring(m.id)]
						body = ui.column {
							class = "rd-page",
							html.remote and not html.allowed and ui.row {
								class = "rd-remote",
								align = "center",
								ui.icon { name = "image", size = "small" },
								ui.row { grow = true, ui.text { "Remote images are blocked to protect your privacy." } },
								ui.row {
									class = "rd-remote-load",
									msg = { remote = m.id },
									to = "provider:vault",
									ui.text { "Load remote images" },
								},
							},
							away and ui.row {
								class = "rd-remote",
								align = "center",
								ui.icon { name = "external-link", size = "small" },
								ui.row { grow = true, ui.text { away, truncate = true } },
								ui.row { class = "rd-remote-load", command = "browser-open", browser = id, ui.text { "Open in browser" } },
								ui.row { class = "rd-remote-load", command = "browser-back", browser = id, ui.text { "Back to message" } },
							},
							ui.column {
								height = html.height,
								ui.browser {
									id = id,
									url = if app.dark then html.dark else html.light,
									navigate = { page = m.id },
									class = "rd-web",
								},
							},
						}
					else
						body = ui.column {
							class = "rd-text",
							ui.text { if m.body ~= "" then m.body else "This message has no archived body." },
						}
					end
					table.insert(items, ui.column {
						class = "rd-card",
						gap = "none",
						ui.row {
							class = "rd-card-head",
							align = "start",
							msg = if many then { toggle = m.id } else nil,
							avatar { initials = m.initials, hue = m.hue, size = 40 },
							ui.column {
								grow = true,
								gap = "small",
								ui.row {
									align = "end",
									gap = "normal",
									ui.text { m.from, class = "rd-name" },
									m.address ~= "" and ui.text { m.address, class = "rd-muted", truncate = true },
								},
								table.unpack(recipients),
							},
							ui.column {
								align = "end",
								gap = "small",
								shrink = false,
								ui.text { m.date, class = "rd-small" },
								m.starred and ui.icon { name = "star", size = "small", class = "rd-star" },
							},
						},
						body,
						#files > 0 and ui.row { class = "rd-files", wrap = true, table.unpack(files) },
					})
				end
			end
		end

		if not thread.chat then
			-- Built newest first (so the newest pages get the web views): put back in order.
			local ordered = {}
			for i = #items, 1, -1 do
				table.insert(ordered, items[i])
			end
			items = ordered
		end

		local chips = {}
		for _, label in thread.labels do
			table.insert(chips, ui.row {
				class = "chip-round",
				align = "center",
				hug = true,
				style = { background = label.color .. "24" },
				ui.row { width = 6, height = 6, style = { background = label.color, radius = 3 } },
				ui.text { label.title, size = "small", style = { color = label.color } },
			})
		end

		local many_emails = not thread.chat and #messages > 1
		local has_html = false
		for _, m in messages do
			if m.html then
				has_html = true
			end
		end
		return ui.column {
			grow = true,
			scroll = true,
			gap = "none",
			ui.row {
				class = "rd-scroll",
				justify = "center",
				shrink = false,
				ui.column {
					class = "rd-body",
					grow = true,
					max_width = 880,
					ui.column {
						class = "rd-head",
						ui.row {
							align = "start",
							gap = "large",
							ui.row { grow = true, ui.text { thread.subject, class = "rd-subject" } },
							has_html and ui.row {
								class = "rd-tool",
								width = 26,
								height = 26,
								align = "center",
								justify = "center",
								msg = "toggle-rich",
								tooltip = if state.rich then "Show plain text (⌘⇧H)" else "Show formatted (⌘⇧H)",
								ui.icon { name = if state.rich then "code" else "file-text", size = "small" },
							},
							many_emails and ui.row {
								class = "rd-tool",
								width = 26,
								height = 26,
								align = "center",
								justify = "center",
								msg = "expand-all",
								tooltip = "Expand or collapse all (⌘;)",
								ui.icon { name = "mails", size = "small" },
							},
						},
						ui.row {
							align = "center",
							gap = "normal",
							wrap = true,
							#chips > 0 and ui.row { gap = "normal", table.unpack(chips) },
							ui.text { thread.meta, class = "rd-meta" },
						},
					},
					ui.column { gap = if thread.chat then "small" else "normal", table.unpack(items) },
				},
			},
			toast,
		}
	end,
}
