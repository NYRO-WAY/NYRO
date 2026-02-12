--
-- NYRO Admin: Backends CRUD
--

local store  = require("nyro.store")
local helper = require("nyro.admin.helper")

return helper.build_crud({
    resource_name = "backend",
    get_all       = store.get_backends,
    get_by_name   = store.get_backend_by_name,
    put           = store.put_backend,
    delete        = store.delete_backend,
    validate      = function(data)
        if not data.endpoints or type(data.endpoints) ~= "table" or #data.endpoints == 0 then
            return nil, "endpoints is required and must be a non-empty array"
        end
        return true
    end,
})
