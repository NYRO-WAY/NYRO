--
-- NYRO Admin: Routes CRUD
--

local store  = require("nyro.store")
local helper = require("nyro.admin.helper")

return helper.build_crud({
    resource_name = "route",
    get_all       = store.get_routes,
    get_by_name   = store.get_route_by_name,
    put           = store.put_route,
    delete        = store.delete_route,
    validate      = function(data)
        if not data.paths or type(data.paths) ~= "table" or #data.paths == 0 then
            return nil, "paths is required and must be a non-empty array"
        end
        return true
    end,
})
