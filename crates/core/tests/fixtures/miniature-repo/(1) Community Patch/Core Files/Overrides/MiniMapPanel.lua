-- fixture: the Community Patch's own MiniMapPanel.lua, where upstream ships it
local g_uiAddins = {};

-- Hooks into g_uiAddins so anything added to it is loaded right away
setmetatable(g_uiAddins, {
	__newindex = function(_, _, value)
		ContextPtr:LoadNewContext(value);
	end,
});

-- Modpacks will add "g_uiAddins[#g_uiAddins + 1] = fileName" lines below
