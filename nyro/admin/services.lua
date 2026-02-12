--
-- NYRO Admin: Services CRUD
--

local store  = require("nyro.store")
local helper = require("nyro.admin.helper")

return helper.build_crud({
    resource_name = "service",
    get_all       = store.get_services,
    get_by_name   = store.get_service_by_name,
    put           = store.put_service,
    delete        = store.delete_service,
    validate      = function(data)
        -- service 必须有 url 或 backend
        if not data.url and not data.backend then
            return nil, "either 'url' or 'backend' is required"
        end
        return true
    end,
})
