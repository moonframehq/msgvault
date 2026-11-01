-- A person or chat as msgvault-gpui draws it (gpui-kit's Avatar): a round
-- tint with the name's initials and a ring, in one of twelve hues the
-- provider picks from the initials. Small ones keep only the first letter
-- when two wide letters would crowd the circle.

local NARROW = { I = true, ["1"] = true }

return component {
	title = "Vault Avatar",
	category = "Data",
	icon = "user",
	description = "A round avatar with initials in one of twelve hues, as msgvault draws people.",
	when = "People and chats in msgvault's list and reader. For other apps use `avatar`.",
	props = {
		initials = "one or two letters",
		hue = "0 to 11: the colour (classes .av-0 to .av-11 in style.luau)",
		size = "pixels (default 34)",
	},
	example = { initials = "PR", hue = 0, size = 40 },
	build = function(props)
		local size = props.size or 34
		local text: string = props.initials or "?"
		local letters = {}
		for _, code in utf8.codes(text) do
			table.insert(letters, utf8.char(code))
		end
		if #letters > 1 and size < 38 then
			local width = 0
			for i = 1, 2 do
				width += if NARROW[letters[i]] then 0.3 else 0.68
			end
			if width * size * 0.5 > size * 0.62 then
				text = letters[1]
			end
		end
		return ui.row {
			class = { "av", `av-{props.hue or 0}` },
			width = size,
			height = size,
			align = "center",
			justify = "center",
			shrink = false,
			ui.text { text, size = if size >= 38 then "title" else "large" },
		}
	end,
}
