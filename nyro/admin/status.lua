--
-- NYRO Admin: Status endpoint
--
-- GET /status — 节点状态 (uptime, version, mode, connections)
--

local ngx   = ngx
local json  = require("cjson.safe")
local store = require("nyro.store")

local _M = {}

-- 启动时间戳 (模块加载时记录)
local start_time = ngx.time()

local function json_response(code, body)
    ngx.status = code
    ngx.header["Content-Type"] = "application/json; charset=utf-8"
    ngx.say(json.encode(body))
    ngx.exit(code)
end

function _M.get()
    local now = ngx.time()
    local uptime = now - start_time

    return json_response(200, {
        code = 0,
        data = {
            version = "master",
            store_mode = store.get_mode(),
            config_version = store.get_version(),
            uptime_seconds = uptime,
            worker_count = ngx.worker.count(),
            worker_id = ngx.worker.id(),
            nginx_version = ngx.config.nginx_version,
        }
    })
end

return _M
