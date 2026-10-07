function PLUGIN:PostInstall(ctx)
  os.execute(string.format('chmod +x "%s/bin/spring"', ctx.rootPath))
end
