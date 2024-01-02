# Shebang

You can specify a tool and its version in a shebang without needing to first
setup `.tool-versions`/`.mise.toml` config:

```typescript
#!/usr/bin/env -S mise x node@20 -- node
// "env -S" allows multiple arguments in a shebang
console.log(`Running node: ${process.version}`);
```

This can also be useful in environments where mise isn't activated
(such as a non-interactive session).
