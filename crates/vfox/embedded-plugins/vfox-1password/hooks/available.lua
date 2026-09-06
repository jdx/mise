local http = require("http")

--- Get the available version list.
--- @param ctx table Empty table, no data provided. Always {}.
--- @return table Version list
function PLUGIN:Available(ctx)
	local result = {}

	-- Fetch CLI v2 versions (current)
	local resp2 = http.get({
		url = "https://app-updates.agilebits.com/product_history/CLI2",
	})
	if resp2.status_code == 200 then
		-- Versions appear on lines after <h3> tags
		-- Pattern: <h3>\n\t\t\t\tVERSION
		for version in resp2.body:gmatch("<h3>[^<]*</h3>%s*([%d%.%-beta]+)") do
			local clean_version = version:match("^%s*(.-)%s*$")
			if clean_version and clean_version ~= "" then
				table.insert(result, {
					version = clean_version,
					note = "CLI v2",
				})
			end
		end
		-- Alternative pattern if the above doesn't work
		if #result == 0 then
			for version in resp2.body:gmatch("<h3>%s*</h3>%s*([%d%.%-beta]+)") do
				local clean_version = version:match("^%s*(.-)%s*$")
				if clean_version and clean_version ~= "" then
					table.insert(result, {
						version = clean_version,
						note = "CLI v2",
					})
				end
			end
		end
		-- Try line-by-line parsing
		if #result == 0 then
			local in_h3 = false
			for line in resp2.body:gmatch("[^\r\n]+") do
				if line:match("<h3>") then
					in_h3 = true
				elseif in_h3 then
					local version = line:match("^%s*([%d%.%-beta]+)%s*$")
					if version then
						table.insert(result, {
							version = version,
							note = "CLI v2",
						})
					end
					in_h3 = false
				end
			end
		end
	end

	-- Fetch CLI v1 versions (legacy)
	local resp1 = http.get({
		url = "https://app-updates.agilebits.com/product_history/CLI",
	})
	if resp1.status_code == 200 then
		local in_h3 = false
		for line in resp1.body:gmatch("[^\r\n]+") do
			if line:match("<h3>") then
				in_h3 = true
			elseif in_h3 then
				local version = line:match("^%s*([%d%.%-beta]+)%s*$")
				if version then
					table.insert(result, {
						version = version,
						note = "CLI v1",
					})
				end
				in_h3 = false
			end
		end
	end

	-- Sort versions (newest first)
	table.sort(result, function(a, b)
		return compare_versions(a.version, b.version)
	end)

	return result
end

--- Compare two version strings
--- @param v1 string
--- @param v2 string
--- @return boolean true if v1 > v2
function compare_versions(v1, v2)
	local function parse(v)
		local parts = {}
		-- Handle beta versions: 2.31.0-beta.01 -> {2, 31, 0, -1, 1}
		local main, beta = v:match("^([%d%.]+)%-beta%.?(%d*)$")
		if main then
			for num in main:gmatch("(%d+)") do
				table.insert(parts, tonumber(num))
			end
			table.insert(parts, -1) -- beta marker
			if beta and beta ~= "" then
				table.insert(parts, tonumber(beta))
			else
				table.insert(parts, 0)
			end
		else
			for num in v:gmatch("(%d+)") do
				table.insert(parts, tonumber(num))
			end
		end
		return parts
	end

	local p1, p2 = parse(v1), parse(v2)
	for i = 1, math.max(#p1, #p2) do
		local n1, n2 = p1[i] or 0, p2[i] or 0
		if n1 ~= n2 then
			return n1 > n2
		end
	end
	return false
end
