# Settings

<script setup>
import { data } from './settings.data.ts'
</script>

The following is a list of all of mise's settings. These can be set via `mise settings set`,
by directly modifying `~/.config/mise/config.toml` or local config, or via environment variables.

Some of them also can be set via global CLI flags.

<div v-for="setting in data.filter(f => f.type)">
  <h2>
    <code>{{ setting.key }}</code>
    <span v-if="setting.deprecated" class="VPBadge warning">deprecated</span>
  </h2>

  <ul>
    <li>
      Type: <code>{{ setting.type }}</code>
      <span v-if="setting.optional">(optional)</span>
    </li>
    <li>Env: <code>{{ setting.env }}</code></li>
    <li>Default: <code>{{ setting.default }}</code></li>
    <li v-if="setting.enum">Choices:
      <ul>
        <li v-for="choice in setting.enum"><code>{{ choice[0] }}</code> – {{choice[1]}}</li>
      </ul>
    </li>
  </ul>

  <span v-html="setting.docs"></span>
</div>

<div v-for="child in data.filter(f => !f.type)">
  <h2>
    <code>{{ child.key }}</code>
  </h2>
  <div v-for="setting in child.settings">
    <h3>
      <code>{{ setting.key }}</code>
      <span v-if="setting.deprecated" class="VPBadge warning">deprecated</span>
    </h3>
    <ul>
      <li>
        Type: <code>{{ setting.type }}</code>
        <span v-if="setting.optional">(optional)</span>
      </li>
      <li>Env: <code>{{ setting.env }}</code></li>
      <li>Default: <code>{{ setting.default }}</code></li>
      <li v-if="setting.enum">Choices:
        <ul>
          <li v-for="choice in setting.enum"><code>{{ choice[0] }}</code> – {{choice[1]}}</li>
        </ul>
      </li>
    </ul>
    <span v-html="setting.docs"></span>
  </div>
</div>
