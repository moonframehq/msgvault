-- An attachment as msgvault's reader shows it: a picture's thumbnail, or an
-- icon by type, with its name and size and Open and Save. Open hands it to
-- the system's app for it; Save writes it to Downloads. Both go to the vault
-- provider, which reads the bytes from the archive.

return component {
	title = "Vault File",
	category = "Data",
	icon = "paperclip",
	description = "An attachment card: thumbnail or type icon, name, size, Open and Save.",
	when = "Attachments in msgvault's reader and its attachments browser. Elsewhere use `record-card` or a row of your own.",
	props = {
		file = "{ id, name, size, icon, thumb? (a picture's path in the image cache) }",
		width = "pixels (default 300)",
		large = "a big preview, for the attachments browser's detail",
	},
	example = { file = { id = "s1", name = "atlas-roadmap-q3.pdf", size = "614 B", icon = "file-text" } },

	styles = {
		[".vf"] = { border = true, border_color = "border", radius = 10, background = "background" },
		[".vf:hover"] = { border_color = "muted-foreground/40" },
		[".vf-thumb"] = { radius = { 9, 9, 0, 0 }, background = "muted" },
		[".vf-thumb-large"] = { radius = 10, background = "muted" },
		[".vf-foot"] = { padding = { 8, 6, 8, 10 }, gap = 10 },
		[".vf-tile"] = { background = "muted", radius = 6, color = "accent" },
		[".vf-big-tile"] = { background = "muted", radius = 16, color = "accent" },
		[".vf-name"] = { weight = "medium" },
		[".vf-size"] = { size = "small", color = "muted-foreground" },
		[".vf-button"] = { radius = 6, color = "muted-foreground" },
		[".vf-button:hover"] = { background = "list-hover", color = "foreground" },
	},

	build = function(props)
		local file = props.file
		local width = props.width or 300
		local function button(icon: string, tooltip: string, msg: any)
			return ui.row {
				class = "vf-button",
				width = 26,
				height = 26,
				align = "center",
				justify = "center",
				shrink = false,
				msg = msg,
				to = "provider:vault",
				tooltip = tooltip,
				ui.icon { name = icon :: any, size = "small" },
			}
		end
		local foot = ui.row {
			class = "vf-foot",
			align = "center",
			not file.thumb and ui.row {
				class = "vf-tile",
				width = 32,
				height = 32,
				align = "center",
				justify = "center",
				shrink = false,
				ui.icon { name = file.icon :: any },
			},
			ui.column {
				grow = true,
				gap = "none",
				ui.text { file.name, class = "vf-name", truncate = true },
				ui.text { file.size, class = "vf-size" },
			},
			button("external-link", "Open in its app", { open_file = file.id }),
			button("download", "Save to Downloads", { save_file = file.id }),
		}
		if props.large then
			return ui.column {
				gap = "large",
				if file.thumb
					then ui.image { src = file.thumb, height = 300, fit = "contain", class = "vf-thumb-large" }
					else ui.row {
						height = 220,
						class = "vf-thumb-large",
						align = "center",
						justify = "center",
						ui.row {
							class = "vf-big-tile",
							width = 88,
							height = 88,
							align = "center",
							justify = "center",
							ui.icon { name = file.icon :: any, size = "title" },
						},
					},
			}
		end
		return ui.column {
			class = "vf",
			width = width,
			gap = "none",
			msg = { open_file = file.id },
			to = "provider:vault",
			tooltip = `Open {file.name}`,
			file.thumb and ui.image { src = file.thumb, width = width - 2, height = 128, fit = "cover", class = "vf-thumb" },
			foot,
		}
	end,
}
