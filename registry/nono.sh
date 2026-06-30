backends = ["github:always-further/nono"]
description = "Sandbox any AI agent in seconds - zero setup, zero latency."
test = { cmd = "nono --version", expected = "nono {{version}}" }
