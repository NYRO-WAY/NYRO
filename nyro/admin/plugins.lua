--
-- NYRO Admin: Global Plugins CRUD
--

local store  = require("nyro.store")
local helper = require("nyro.admin.helper")

return helper.build_crud({
    resource_name = "plugin",
    get_all       = store.get_plugins,
    get_by_name   = store.get_plugin_by_name,
    put           = store.put_plugin,
    delete        = store.delete_plugin,
})
