-- The sidebar: the account in view, the standard mailboxes, coloured user
-- labels, Gmail's categories, the archive's other sources (chats, the
-- calendar), the accounts, and the archive's size at the foot.

type Box = { id: string, title: string, icon: any, count: string?, strong: boolean? }

local MAILBOXES: { Box } = {
	{ id = "inbox", title = "Inbox", icon = "inbox", count = "inbox", strong = true },
	{ id = "starred", title = "Starred", icon = "star", count = "starred" },
	{ id = "unread", title = "Unread", icon = "mail-open" },
	{ id = "important", title = "Important", icon = "flag" },
	{ id = "sent", title = "Sent", icon = "send" },
	{ id = "drafts", title = "Drafts", icon = "file", count = "drafts" },
	{ id = "all", title = "All Mail", icon = "mails", count = "all" },
	{ id = "attachments", title = "Attachments", icon = "paperclip", count = "attachments" },
	{ id = "spam", title = "Spam", icon = "octagon-alert" },
	{ id = "trash", title = "Trash", icon = "trash" },
}

local function number(n: number): string
	local text = tostring(math.floor(n))
	while true do
		local next, k = string.gsub(text, "^(%d+)(%d%d%d)", "%1,%2")
		text = next
		if k == 0 then
			return text
		end
	end
end

return mod {
	region = "left",
	title = "Sidebar",

	styles = {
		[".sb-head"] = { padding = { 16, 16, 6, 17 }, gap = 9 },
		[".sb-tile"] = { background = "accent", color = "accent-foreground", radius = 8 },
		[".sb-title"] = { weight = "bold" },
		[".sb-small"] = { size = "small", color = "muted-foreground" },
		[".sb-body"] = { padding = { 0, 8, 8, 8 } },
		[".sb-group"] = { size = "small", color = "muted-foreground", padding = { 8, 9, 3, 9 } },
		[".sb-row"] = { padding = { 0, 9 }, radius = 6, gap = 8 },
		[".sb-row:hover"] = { background = "list-hover" },
		[".sb-row:selected"] = { background = "side-active", weight = "medium" },
		[".sb-icon"] = { color = "foreground" },
		[".sb-count"] = { size = "small", color = "muted-foreground", padding = { 0, 6 } },
		[".sb-pill"] = { size = "small", weight = "bold", background = "accent", color = "accent-foreground", radius = 999, padding = { 0, 6 } },
		[".sb-foot"] = { padding = { 10, 16, 12, 16 }, gap = 2, size = "small", color = "muted-foreground" },
	},

	view = function(_, app)
		local vault = app.vault
		if not vault or not vault.available then
			return ui.column { grow = true }
		end
		local searching = vault.query ~= ""

		local function row(id: string, title: string, icon: any, count: number?, strong: boolean?, tint: string?, msg: any)
			local suffix = nil
			if count and count > 0 then
				suffix = ui.row {
					class = if strong then "sb-pill" else "sb-count",
					width = 30,
					height = 18,
					align = "center",
					justify = "center",
					shrink = false,
					ui.text { number(count) },
				}
			end
			return ui.row {
				class = "sb-row",
				height = 31,
				align = "center",
				selected = not searching and vault.mailbox == id,
				msg = msg or { mailbox = id },
				to = "provider:vault",
				ui.icon { name = icon, class = if tint then nil else "sb-icon", style = if tint then { color = tint } else nil },
				ui.text { title, truncate = true },
				ui.spacer(),
				suffix,
			}
		end

		local rows: { any } = { ui.text { "Mailboxes", class = "sb-group" } }
		for _, box in MAILBOXES do
			table.insert(rows, row(box.id, box.title, box.icon, if box.count then vault.counts[box.count] else nil, box.strong))
		end
		if vault.total_files > 0 then
			table.insert(rows, ui.text { "Files", class = "sb-group" })
			table.insert(rows, row("files", "All Attachments", "folder-open", vault.total_files))
		end
		if #vault.labels > 0 then
			table.insert(rows, ui.text { "Labels", class = "sb-group" })
			for _, label in vault.labels do
				table.insert(rows, row("label:" .. label.name, label.title, "tag", label.count, false, label.color))
			end
		end
		if #vault.categories > 0 then
			table.insert(rows, ui.text { "Categories", class = "sb-group" })
			for _, label in vault.categories do
				table.insert(rows, row("label:" .. label.name, label.title, "tags", label.count))
			end
		end
		if #vault.archive > 0 then
			table.insert(rows, ui.text { "Archive", class = "sb-group" })
			for _, kind in vault.archive do
				table.insert(rows, row(kind.id, kind.title, kind.icon, kind.count))
			end
		end
		table.insert(rows, ui.text { "Accounts", class = "sb-group" })
		local total = 0
		for _, account in vault.accounts do
			total += account.count
		end
		local function account_row(id: number, title: string, icon: any, count: number)
			return ui.row {
				class = "sb-row",
				height = 31,
				align = "center",
				selected = vault.account == id,
				msg = { account = id },
				to = "provider:vault",
				ui.icon { name = icon, class = "sb-icon" },
				ui.text { title, truncate = true },
				ui.spacer(),
				ui.row { class = "sb-count", height = 18, align = "center", shrink = false, ui.text { number(count) } },
			}
		end
		table.insert(rows, account_row(0, "All Accounts", "users", total))
		for _, account in vault.accounts do
			table.insert(rows, account_row(account.id, account.title, account.icon, account.count))
		end

		local header = vault.header
		return ui.column {
			grow = true,
			gap = "none",
			ui.row {
				class = "sb-head",
				align = "center",
				shrink = false,
				ui.row {
					class = "sb-tile",
					width = 28,
					height = 28,
					align = "center",
					justify = "center",
					shrink = false,
					ui.icon { name = header.icon :: any },
				},
				ui.column {
					gap = "none",
					grow = true,
					ui.text { header.title, class = "sb-title", truncate = true },
					ui.text { header.subtitle, class = "sb-small", truncate = true },
				},
			},
			ui.column {
				grow = true,
				scroll = true,
				gap = "none",
				ui.column { class = "sb-body", gap = "none", shrink = false, table.unpack(rows) },
			},
			ui.column {
				class = "sb-foot",
				shrink = false,
				ui.text { `{vault.stats.messages} · {vault.stats.threads}` },
				ui.text { `{vault.stats.attachments} · {vault.stats.size} on disk` },
			},
		}
	end,
}
