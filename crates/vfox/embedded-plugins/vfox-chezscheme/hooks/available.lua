local http = require("http")
local json = require("json")

--- Returns all available versions of Chez Scheme from GitHub tags.
--- @param ctx table Context
--- @return table Available versions
function PLUGIN:Available(ctx)
	local results = {}

	-- Fetch releases from GitHub API (tags show up as releases)
	local resp, err = http.get({
		url = "https://api.github.com/repos/cisco/ChezScheme/tags?per_page=100",
	})

	if err ~= nil then
		error("Failed to fetch versions: " .. err)
	end

	if resp.status_code ~= 200 then
		error("Failed to fetch versions: HTTP " .. resp.status_code)
	end

	local tags = json.decode(resp.body)

	for _, tag in ipairs(tags) do
		local version = tag.name
		-- Remove 'v' prefix if present
		if version:sub(1, 1) == "v" then
			version = version:sub(2)
		end
		-- Only include version-like tags (e.g., 10.3.0, 9.5.8)
		if version:match("^%d+%.%d+") then
			table.insert(results, {
				version = version,
				note = "",
			})
		end
	end

	return results
end
