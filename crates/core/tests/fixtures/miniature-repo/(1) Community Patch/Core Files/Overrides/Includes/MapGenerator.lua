-- fixture: the Community Patch's own MapGenerator.lua, where upstream ships it
local g_uiAddins = {};

function RunAddins()
	for _, file in ipairs(g_uiAddins) do
		include(file);
	end
end

-- Modpacks will add "g_uiAddins[#g_uiAddins + 1] = fileName" lines below
