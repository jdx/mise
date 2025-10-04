--- Get the available version list.
--- @param ctx table Empty table, no data provided. Always {}.
--- @return table Version list
function PLUGIN:Available(ctx)
    return {
        {
            version = "1.0.0"
        },
        {
            version = "1.0.1"
        },
    }
end