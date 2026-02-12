--
-- NYRO Admin CRUD Helper
--
-- 为 6 种资源提供通用 CRUD 逻辑，减少重复代码
--

local ngx = ngx
local json = require("cjson.safe")
local store = require("nyro.store")

local _M = {}

-- Admin API 读操作前同步: 从文件刷新内存确保跨 worker 一致性
local function sync_store()
    if store.reload then
        store.reload()
    end
end

-- 统一 JSON 响应
function _M.json_response(code, body)
    ngx.status = code
    ngx.header["Content-Type"] = "application/json; charset=utf-8"
    ngx.say(json.encode(body))
    ngx.exit(code)
end

-- 构建通用 CRUD handler
-- opts:
--   resource_name: 资源类型名 (如 "route")
--   get_all:       store.get_xxx 函数
--   get_by_name:   store.get_xxx_by_name 函数
--   put:           store.put_xxx 函数
--   delete:        store.delete_xxx 函数
--   validate:      可选的校验函数 (data) → true/nil, err_message
function _M.build_crud(opts)
    local handler = {}

    -- GET list
    function handler.list()
        sync_store()
        local items, err = opts.get_all()
        if err then
            return _M.json_response(500, { code = 500, message = err })
        end

        -- 清理 _index 等内部字段
        local clean = {}
        for i, item in ipairs(items or {}) do
            clean[i] = item
        end

        return _M.json_response(200, {
            code = 0,
            data = {
                total = #clean,
                items = clean,
            }
        })
    end

    -- GET by name
    function handler.get(name)
        sync_store()
        local item = opts.get_by_name(name)
        if not item then
            return _M.json_response(404, {
                code = 404,
                message = opts.resource_name .. " not found: " .. name,
            })
        end

        return _M.json_response(200, {
            code = 0,
            data = item,
        })
    end

    -- POST create
    function handler.create(body)
        if not body.name then
            return _M.json_response(400, {
                code = 400,
                message = "name is required",
            })
        end

        -- 刷新内存确保跨 worker 一致性
        sync_store()

        -- 检查是否已存在
        local existing = opts.get_by_name(body.name)
        if existing then
            return _M.json_response(409, {
                code = 409,
                message = opts.resource_name .. " already exists: " .. body.name,
            })
        end

        -- 自定义校验
        if opts.validate then
            local ok, err = opts.validate(body)
            if not ok then
                return _M.json_response(400, { code = 400, message = err })
            end
        end

        local ok, err = opts.put(body.name, body)
        if not ok then
            return _M.json_response(400, { code = 400, message = err })
        end

        ngx.log(ngx.INFO, "[admin] created ", opts.resource_name, ": ", body.name)
        return _M.json_response(201, {
            code = 0,
            message = "created",
            data = body,
        })
    end

    -- PUT update (full replace)
    function handler.update(name, body)
        -- 确保 body.name 与 URL 中的 name 一致
        body.name = name

        -- 自定义校验
        if opts.validate then
            local ok, err = opts.validate(body)
            if not ok then
                return _M.json_response(400, { code = 400, message = err })
            end
        end

        local ok, err = opts.put(name, body)
        if not ok then
            return _M.json_response(400, { code = 400, message = err })
        end

        ngx.log(ngx.INFO, "[admin] updated ", opts.resource_name, ": ", name)
        return _M.json_response(200, {
            code = 0,
            message = "updated",
            data = body,
        })
    end

    -- DELETE
    function handler.delete(name)
        local ok, err = opts.delete(name)
        if not ok then
            return _M.json_response(400, { code = 400, message = err })
        end

        ngx.log(ngx.INFO, "[admin] deleted ", opts.resource_name, ": ", name)
        return _M.json_response(200, {
            code = 0,
            message = "deleted",
        })
    end

    return handler
end

return _M
