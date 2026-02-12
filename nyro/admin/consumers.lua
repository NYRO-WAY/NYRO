--
-- NYRO Admin: Consumers CRUD
--

local store  = require("nyro.store")
local helper = require("nyro.admin.helper")

return helper.build_crud({
    resource_name = "consumer",
    get_all       = store.get_consumers,
    get_by_name   = store.get_consumer_by_name,
    put           = store.put_consumer,
    delete        = store.delete_consumer,
})
