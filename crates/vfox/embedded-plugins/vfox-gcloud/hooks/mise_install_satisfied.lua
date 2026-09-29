local components = require("components")

--- Tell mise whether an installed gcloud still has every component requested
--- by the `components` tool option. When one is missing, mise reruns
--- PostInstall on the existing SDK instead of downloading it again.
--- Requires a mise version with MiseInstallSatisfied support; older versions
--- ignore this hook.
--- @param ctx MiseInstallSatisfiedCtx
--- @return MiseInstallSatisfiedResult
function PLUGIN:MiseInstallSatisfied(ctx)
    local requested = components.from_options(ctx.options)
    local missing = components.missing(ctx.path, requested)
    if #missing > 0 then
        return {
            satisfied = false,
            reason = "missing Cloud SDK components: " .. table.concat(missing, ", "),
        }
    end
    return { satisfied = true }
end
