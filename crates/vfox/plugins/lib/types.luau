--!strict
--[[
    Type definitions for mise plugin runtime.
    Usage: local Types = require("lib/types")

    Documentation: https://mise.jdx.dev/dev-tools/backends/vfox.html
]]

export type SdkInfo = {
	name: string,
	version: string,
	path: string,
	note: string?,
}

export type AvailableResult = {
	version: string,
	note: string?,
	addition: { { name: string, version: string? } }?,
}

export type PreInstallResult = {
	version: string,
	url: string?,
	sha256: string?,
	sha512: string?,
	md5: string?,
	note: string?,
	addition: { { name: string, url: string, sha256: string?, sha512: string?, md5: string? } }?,
}

export type EnvKeyResult = {
	key: string,
	value: string,
}

export type ParseLegacyFileResult = {
	version: string?,
}

export type PluginType = {
	name: string,
	version: string?,
	description: string?,
	author: string?,
	license: string?,
	homepage: string?,
	minRuntimeVersion: string?,
	notes: { string }?,
	legacyFilenames: { string }?,

	Available: ((self: PluginType, ctx: { args: { string }? }) -> { AvailableResult })?,
	PreInstall: ((self: PluginType, ctx: { version: string, runtimeVersion: string }) -> PreInstallResult)?,
	PostInstall: ((
		self: PluginType,
		ctx: { rootPath: string, runtimeVersion: string, sdkInfo: { [string]: SdkInfo } }
	) -> ())?,
	EnvKeys: ((
		self: PluginType,
		ctx: { path: string, runtimeVersion: string, sdkInfo: { [string]: SdkInfo }? }
	) -> { EnvKeyResult })?,
	ParseLegacyFile: ((self: PluginType, ctx: { filepath: string, filename: string? }) -> ParseLegacyFileResult)?,

	-- Backend plugin hooks
	BackendListVersions: ((self: PluginType, ctx: { tool: string }) -> { versions: { string } })?,
	BackendInstall: ((self: PluginType, ctx: { tool: string, version: string, install_path: string }) -> {})?,
	BackendExecEnv: ((
		self: PluginType,
		ctx: { install_path: string, tool: string, version: string }
	) -> { env_vars: { EnvKeyResult } })?,

	-- Env plugin hooks
	MiseEnv: ((self: PluginType, ctx: { env: { [string]: string } }) -> { [string]: string })?,
	MisePath: ((self: PluginType, ctx: { env: { [string]: string } }) -> { string })?,
}

export type RuntimeType = {
	osType: string,
	archType: string,
	version: string,
}

-- HTTP Module
export type HttpRequestOptions = {
	url: string,
	headers: { [string]: string }?,
}

export type HttpResponse = {
	status_code: number,
	headers: { [string]: string },
	body: string?,
}

export type HttpModule = {
	get: (opts: HttpRequestOptions) -> HttpResponse,
	head: (opts: HttpRequestOptions) -> HttpResponse,
	download_file: (opts: HttpRequestOptions, path: string) -> (),
}

-- JSON Module
export type JsonModule = {
	encode: (value: any) -> string,
	decode: (str: string) -> any,
}

-- File Module
export type FileModule = {
	read: (path: string) -> string,
	exists: (path: string) -> boolean,
	symlink: (src: string, dst: string) -> (),
	join_path: (...string) -> string,
}

-- Cmd Module
export type CmdOptions = {
	cwd: string?,
	env: { [string]: string }?,
	timeout: number?,
}

export type CmdModule = {
	exec: ((command: string) -> string) & ((command: string, options: CmdOptions) -> string),
}

-- Env Module
export type EnvModule = {
	getenv: (key: string) -> string?,
	setenv: (key: string, value: string) -> (),
}

-- Log Module
export type LogModule = {
	trace: (...any) -> (),
	debug: (...any) -> (),
	info: (...any) -> (),
	warn: (...any) -> (),
	error: (...any) -> (),
}

-- Archiver Module
export type ArchiverModule = {
	decompress: (archive: string, destination: string) -> (),
}

-- Semver Module
export type SemverModule = {
	parse: (version: string) -> { number },
	compare: (v1: string, v2: string) -> number,
	sort: (versions: { string }) -> { string },
}

-- Strings Module
export type StringsModule = {
	split: (s: string, sep: string) -> { string },
	has_prefix: (s: string, prefix: string) -> boolean,
	has_suffix: (s: string, suffix: string) -> boolean,
	trim: (s: string, cutset: string) -> string,
	trim_space: (s: string) -> string,
	contains: (s: string, substr: string) -> boolean,
	join: (arr: { any }, sep: string) -> string,
}

return {}
