--
-- NYRO Admin API
--
-- RESTful API 路由分发: 解析 method + path → handler
-- 端点前缀: /nyro/admin
--

local ngx = ngx
local json = require("cjson.safe")

local routes_handler       = require("nyro.admin.routes")
local services_handler     = require("nyro.admin.services")
local backends_handler     = require("nyro.admin.backends")
local consumers_handler    = require("nyro.admin.consumers")
local plugins_handler      = require("nyro.admin.plugins")
local certificates_handler = require("nyro.admin.certificates")
local config_handler       = require("nyro.admin.config")
local status_handler       = require("nyro.admin.status")

local _M = {}

-- 统一 JSON 响应
local function json_response(code, body)
    ngx.status = code
    ngx.header["Content-Type"] = "application/json; charset=utf-8"
    ngx.say(json.encode(body))
    ngx.exit(code)
end

-- 读取 JSON 请求体
local function read_json_body()
    ngx.req.read_body()
    local body = ngx.req.get_body_data()
    if not body or body == "" then
        return nil, "empty request body"
    end

    local data = json.decode(body)
    if not data then
        return nil, "invalid JSON body"
    end

    return data
end

-- 资源 handler 映射
local resource_handlers = {
    routes       = routes_handler,
    services     = services_handler,
    backends     = backends_handler,
    consumers    = consumers_handler,
    plugins      = plugins_handler,
    certificates = certificates_handler,
}

-- 主分发入口
function _M.dispatch()
    local method = ngx.req.get_method()
    local uri = ngx.var.uri

    -- 去掉前缀 /nyro/admin
    local path = uri:match("^/nyro/admin(.*)$")
    if not path or path == "" then
        path = "/"
    end

    -- 解析 path: /resource 或 /resource/name
    local resource, name = path:match("^/([%w_-]+)/?(.-)$")
    if name == "" then
        name = nil
    end

    -- config 端点: POST /config/reload, GET /config/version
    if resource == "config" then
        if method == "POST" and name == "reload" then
            return config_handler.reload()
        elseif method == "GET" and name == "version" then
            return config_handler.version()
        else
            return json_response(404, { code = 404, message = "not found" })
        end
    end

    -- status 端点
    if resource == "status" then
        if method == "GET" then
            return status_handler.get()
        else
            return json_response(405, { code = 405, message = "method not allowed" })
        end
    end

    -- 资源 CRUD
    local handler = resource_handlers[resource]
    if not handler then
        return json_response(404, { code = 404, message = "unknown resource: " .. tostring(resource) })
    end

    if method == "GET" then
        if name then
            return handler.get(name)
        else
            return handler.list()
        end
    elseif method == "POST" then
        local body, err = read_json_body()
        if not body then
            return json_response(400, { code = 400, message = err })
        end
        return handler.create(body)
    elseif method == "PUT" then
        if not name then
            return json_response(400, { code = 400, message = "name is required in URL" })
        end
        local body, err = read_json_body()
        if not body then
            return json_response(400, { code = 400, message = err })
        end
        return handler.update(name, body)
    elseif method == "DELETE" then
        if not name then
            return json_response(400, { code = 400, message = "name is required in URL" })
        end
        return handler.delete(name)
    else
        return json_response(405, { code = 405, message = "method not allowed" })
    end
end

return _M
