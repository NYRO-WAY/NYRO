--
-- NYRO Admin: Certificates CRUD
--

local store  = require("nyro.store")
local helper = require("nyro.admin.helper")

return helper.build_crud({
    resource_name = "certificate",
    get_all       = store.get_certificates,
    get_by_name   = store.get_certificate_by_name,
    put           = store.put_certificate,
    delete        = store.delete_certificate,
    validate      = function(data)
        if not data.snis or type(data.snis) ~= "table" or #data.snis == 0 then
            return nil, "snis is required and must be a non-empty array"
        end
        if not data.cert and not data.cert_file then
            return nil, "either 'cert' or 'cert_file' is required"
        end
        if not data.key and not data.key_file then
            return nil, "either 'key' or 'key_file' is required"
        end
        return true
    end,
})
