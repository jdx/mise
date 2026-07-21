function PLUGIN:PreUninstall(ctx)
	local main = ctx.main
	local marker = main.path .. "/pre_uninstall_marker"
	local marker_file = io.open(marker, "w")
	if marker_file then
		marker_file:write(main.name .. ":" .. main.version .. ":" .. string.gsub(main.path, "\\", "/"))
		marker_file:close()
	end
end
