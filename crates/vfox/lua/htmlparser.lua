-- vim: ft=lua ts=2 sw=2

-- Syntactic Sugar {{{
local function rine(val) -- Return (val) If it's Not Empty (non-zero-length)
	return (val and #val>0) and val
end
local function rit(a) -- Return (a) If it's Table
	return (type(a) == "table") and a
end
local noop = function() end
local esc = function(s) return string.gsub(s, "([%^%$%(%)%%%.%[%]%*%+%-%?])", "%%" .. "%1") end
local str = tostring
local char = string.char
local opts = rit(htmlparser_opts) or {} -- needed for silent/noerr/noout/nonl directives, also needed to be defined before `require` in such case
local prn = opts.silent and noop or function(l,f,...)
	local fd = (l=="i") and "stdout" or "stderr"
	local t = (" [%s] "):format(l:upper())
	io[fd]
		:write('[HTMLParser]'..t..f:format(...)
			..(opts.nonl or "\n")
		)
end
local err = opts.noerr and noop or function(f,...) prn("e",f,...) end
local out = opts.noout and noop or function(f,...) prn("i",f,...) end
local line = debug and function(lvl) return debug.getinfo(lvl or 2).currentline end or noop
local dbg = opts.debug and function(f,...) prn("d",f:gsub("#LINE#",str(line(3))),...) end or noop
-- }}}
-- Requires {{{
local ElementNode = require"htmlparser.ElementNode"
local voidelements = require"htmlparser.voidelements"
--}}}
local HtmlParser = {}
local function parse(text,limit) -- {{{
	local opts = rine(opts) -- use top-level opts-table (the one, defined before requiring the module), if exists
		or rit(htmlparser_opts) -- or defined after requiring (but before calling `parse`)
		or {} -- fallback otherwise
	opts.looplimit = opts.looplimit or htmlparser_looplimit

	local text = str(text)
	local limit = limit or opts.looplimit or 1000
	local tpl = false

	if not opts.keep_comments then -- Strip (or not) comments {{{
		text = text:gsub("<!%-%-.-%-%->","") -- Many chances commented code will have syntax errors, that'll lead to parser failures
	end -- }}}

	local tpr={}

	if not opts.keep_danger_placeholders then -- {{{ little speedup by cost of potential parsing breakages
		-- search unused "invalid" bytes {{{
		local busy,i={},0;
		repeat -- {{{
			local cc = char(i)
			if not(text:match(cc)) then -- {{{
				if not(tpr["<"]) or not(tpr[">"]) then -- {{{
					if not(busy[i]) then -- {{{
						if not(tpr["<"]) then -- {{{
							tpr["<"] = cc;
						elseif not(tpr[">"]) then
							tpr[">"] = cc;
						end -- }}}
						busy[i] = true
						dbg("c:{%s}||cc:{%d}||tpr[c]:{%s}",str(c),cc:byte(),str(tpr[c]))
						dbg("busy[i]:{%s},i:{%d}",str(busy[i]),i)
						dbg("[FindPH]:#LINE# Success! || i=%d",i)
					else -- if !busy
						dbg("[FindPH]:#LINE# Busy! || i=%d",i)
					end -- if !busy -- }}}
					dbg("c:{%s}||cc:{%d}||tpr[c]:{%s}",c,cc:byte(),str(tpr[c]))
					dbg("%s",str(busy[i]))
				else -- if < or >
					dbg("[FindPH]:#LINE# Done!",i)
					break
				end -- if < or > -- }}}
			else -- text!match(cc)
				dbg("[FindPH]:#LINE# Text contains this byte! || i=%d",i)
			end -- text!match(cc) -- }}}
			local skip=1
			if i==31 then
				skip=96 -- ASCII
			end
			i=i+skip
		until (i==255) -- }}}
		i=nil
		--- }}}

		if not(tpr["<"]) or not(tpr[">"]) then
			err("Impossible to find at least two unused byte codes in this HTML-code. We need it to escape bracket-contained placeholders inside tags.")
			err("Consider enabling 'keep_danger_placeholders' option (to silence this error, if parser wasn't failed with current HTML-code) or manually replace few random bytes, to free up the codes.")
		else
			dbg("[FindPH]:#LINE# Found! || '<'=%d, '>'=%d",tpr["<"]:byte(),tpr[">"]:byte())
		end

--	dbg("tpr[>] || tpr[] || #busy%d")

		-- g {{{
		local function g(id,...)
			local arg={...}
			local orig=arg[id]
			arg[id]=arg[id]:gsub("(.)",tpr)
			if arg[id] ~= orig then
				tpl=true
				dbg("[g]:#LINE# orig: %s", str(orig))
				dbg("[g]:#LINE# replaced: %s",str(arg[id]))
			end
			dbg("[g]:#LINE# called, id: %s, arg[id]: %s, args { "..(("{%s}, "):rep(#arg):gsub(", $","")).." }",id,arg[id],...)
			dbg("[g]:#LINE# concat(arg): %s",table.concat(arg))
			return table.concat(arg)
		end
		-- g }}}

		-- tpl-placeholders and attributes {{{
		text=text
			:gsub(
				"(=[%s]-)".. -- only match attr.values, and not random strings between two random apostrophs
				"(%b'')",
				function(...)return g(2,...)end
			)
			:gsub(
				"(=[%s]-)".. -- same for "
				'(%b"")',
				function(...)return g(2,...)end
			) -- Escape "<"/">" inside attr.values (see issue #50)
			:gsub(
				"(<".. -- Match "<",
				(opts.tpl_skip_pattern or "[^!]").. -- with exclusion pattern (for example, to ignore comments, which aren't template placeholders, but can legally contain "<"/">" inside.
				")([^>]+)".. -- If matched, we want to escape '<'s if we meet them inside tag
				"(>)",
				function(...)return g(2,...)end
			)
			:gsub(
				"("..
				(tpr["<"] or "__FAILED__").. -- Here we search for "<", we escaped in previous gsub (and don't break things if we have no escaping replacement)
				")("..
				(opts.tpl_marker_pattern or "[^%w%s]").. -- Capture templating symbol
				")([%g%s]-)".. -- match placeholder's content
				"(%2)(>)".. -- placeholder's tail
				"([^>]*>)", -- remainings
				function(...)return g(5,...)end
			)
		-- }}}
	end -- }}}

	local index = 0
	local root = ElementNode:new(index, str(text))
	local node, descend, tpos, opentags = root, true, 1, {}

	while true do -- MainLoop {{{
		if index == limit then -- {{{
			err("Main loop reached loop limit (%d). Consider either increasing it or checking HTML-code for syntax errors", limit)
			break
		end -- }}}
		-- openstart/tpos Definitions {{{
		local openstart, name
		openstart, tpos, name = root._text:find(
			"<" ..        -- an uncaptured starting "<"
			"([%w-]+)" .. -- name = the first word, directly following the "<"
			"[^>]*>",     -- include, but not capture everything up to the next ">"
		tpos)
		dbg("[MainLoop]:#LINE# openstart=%s || tpos=%s || name=%s",str(openstart),str(tpos),str(name))
		-- }}}
		if not name then break end
		-- Some more vars {{{
		index = index + 1
		local tag = ElementNode:new(index, str(name), (node or {}), descend, openstart, tpos)
		node = tag
		local tagloop
		local tagst, apos = tag:gettext(), 1
		-- }}}
		while true do -- TagLoop {{{
			dbg("[TagLoop]:#LINE# tag.name=%s, tagloop=%s",str(tag.name),str(tagloop))
			if tagloop == limit then -- {{{
				err("Tag parsing loop reached loop limit (%d). Consider either increasing it or checking HTML-code for syntax errors", limit)
				break
			end -- }}}
			-- Attrs {{{
			local start, k, eq, quote, v, zsp
			start, apos, k, zsp, eq, zsp, quote = tagst:find(
				"%s+" ..         -- some uncaptured space
				"([^%s=/>]+)" .. -- k = an unspaced string up to an optional "=" or the "/" or ">"
				"([%s]-)"..      -- zero or more spaces
				"(=?)" ..        -- eq = the optional; "=", else ""
				"([%s]-)"..      -- zero or more spaces
				[=[(['"]?)]=],      -- quote = an optional "'" or '"' following the "=", or ""
			apos)
			dbg("[TagLoop]:#LINE# start=%s || apos=%s || k=%s || zsp='%s' || eq='%s', quote=[%s]",str(start),str(apos),str(k),str(zsp),str(eq),str(quote))
			-- }}}
			if not k or k == "/>" or k == ">" then break end
			-- Pattern {{{
			if eq == "=" then
				local pattern = "=([^%s>]*)"
				if quote ~= "" then
					pattern = quote .. "([^" .. quote .. "]*)" .. quote
				end
				start, apos, v = tagst:find(pattern, apos)
				dbg("[TagLoop]:#LINE# start=%s || apos=%s || v=%s || pattern=%s",str(start),str(apos),str(v),str(pattern))
			end
			-- }}}
			v=v or ""
			if tpl then -- {{{
				for rk,rv in pairs(tpr) do
					v = v:gsub(rv,rk)
					dbg("[TagLoop]:#LINE# rv=%s || rk=%s",str(rv),str(rk))
				end
			end -- }}}

			dbg("[TagLoop]:#LINE# k=%s || v=%s",str(k),str(v))
			tag:addattribute(k, v)
			tagloop = (tagloop or 0) + 1
		end
		-- }}}
		if voidelements[tag.name:lower()] then -- {{{
			descend = false
			tag:close()
		else
			descend = true
			opentags[tag.name] = opentags[tag.name] or {}
			table.insert(opentags[tag.name], tag)
		end
		-- }}}
		local closeend = tpos
		local closingloop
		while true do -- TagCloseLoop {{{
			-- Can't remember why did I add that, so comment it for now (and not remove), in case it will be needed again
			-- (although, it causes #59 and #60, so it will anyway be needed to rework)
			-- if voidelements[tag.name:lower()] then break end -- already closed
			if closingloop == limit then
				err("Tag closing loop reached loop limit (%d). Consider either increasing it or checking HTML-code for syntax errors", limit)
				break
			end

			local closestart, closing, closename
			closestart, closeend, closing, closename = root._text:find("[^<]*<(/?)([%w-]+)", closeend)
			dbg("[TagCloseLoop]:#LINE# closestart=%s || closeend=%s || closing=%s || closename=%s",str(closestart),str(closeend),str(closing),str(closename))

			if not closing or closing == "" then break end

			tag = table.remove(opentags[closename] or {}) or tag -- kludges for the cases of closing void or non-opened tags
			closestart = root._text:find("<", closestart)
			dbg("[TagCloseLoop]:#LINE# closestart=%s",str(closestart))
			tag:close(closestart, closeend + 1)
			node = tag.parent
			descend = true
			closingloop = (closingloop or 0) + 1
		end -- }}}
	end -- }}}
	if tpl then -- {{{
		dbg("tpl")
		for k,v in pairs(tpr) do
			root._text = root._text:gsub(v,k)
		end
	end -- }}}
	return root
end -- }}}
HtmlParser.parse = parse
return HtmlParser
