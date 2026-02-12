--
-- NYRO Admin: Config endpoints
--
-- POST /config/reload  — 手动触发热加载 (重新读取 YAML 文件)
-- GET  /config/version — 获取当前配置版本号
--

local ngx   = ngx
local json  = require("cjson.safe")
local store = require("nyro.store")

local _M = {}

local function json_response(code, body)
    ngx.status = code
    ngx.header["Content-Type"] = "application/json; charset=utf-8"
    ngx.say(json.encode(body))
    ngx.exit(code)
end

function _M.reload()
    local ok, err = store.reload()
    if not ok then
        return json_response(500, {
            code = 500,
            message = "reload failed: " .. tostring(err),
        })
    end

    ngx.log(ngx.INFO, "[admin] config reloaded via Admin API")
    return json_response(200, {
        code = 0,
        message = "reloaded",
        data = {
            version = store.get_version(),
        }
    })
end

function _M.version()
    return json_response(200, {
        code = 0,
        data = {
            version = store.get_version(),
        }
    })
end

return _M
