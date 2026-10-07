function PLUGIN:PostInstall(ctx)
  os.execute(string.format('chmod +x "%s/bin/jib"', ctx.rootPath))
end
