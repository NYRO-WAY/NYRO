--
-- NYRO System Consumer
--
-- 消费者资源管理模块，用于 API 消费者认证
--

local ngx = ngx
local ipairs = ipairs
local pairs = pairs
local type = type
local store = require("nyro.store")
local events = require("resty.worker.events")
local ngx_process = require("ngx.process")
local ngx_timer_at = ngx.timer.at
local ngx_sleep = ngx.sleep
local ngx_worker_exiting = ngx.worker.exiting

-- 消费者数据索引
local consumers = {}
-- 凭证索引: { ["key-auth:mobile-app-key"] = consumer_name }
local credential_index = {}
local current_version = 0

local _M = {}

_M.events_source_consumer = "events_source_consumer"
_M.events_type_rebuild_consumer = "events_type_rebuild_consumer"

-- 从 Store 加载消费者数据
local function load_consumers_from_store()
    if not store.is_initialized() then
        return nil, "store not initialized"
    end

    local items, err = store.get_consumers()
    if err then
        return nil, err
    end

    return items or {}
end

-- 构建凭证索引
local function build_credential_index(items)
    local index = {}
    
    for _, consumer in ipairs(items) do
        if consumer.name and consumer.credentials then
            for cred_type, cred_data in pairs(consumer.credentials) do
                -- 支持不同类型的凭证
                if cred_type == "key-auth" and cred_data.key then
                    index["key-auth:" .. cred_data.key] = consumer.name
                elseif cred_type == "basic-auth" and cred_data.username then
                    index["basic-auth:" .. cred_data.username] = consumer.name
                elseif cred_type == "jwt-auth" and cred_data.key then
                    index["jwt-auth:" .. cred_data.key] = consumer.name
                elseif cred_type == "hmac-auth" and cred_data.key then
                    index["hmac-auth:" .. cred_data.key] = consumer.name
                end
            end
        end
    end
    
    return index
end

-- 重建消费者数据
local function rebuild_consumers()
    local items, err = load_consumers_from_store()
    if err then
        ngx.log(ngx.WARN, "[sys.consumer] failed to load consumers: ", err)
        return false
    end

    -- 构建消费者索引
    local consumer_map = {}
    for _, consumer in ipairs(items) do
        if consumer.name then
            consumer_map[consumer.name] = consumer
        end
    end

    -- 构建凭证索引
    local cred_idx = build_credential_index(items)

    consumers = consumer_map
    credential_index = cred_idx
    current_version = store.get_version()

    ngx.log(ngx.INFO, "[sys.consumer] consumers rebuilt, count: ", #items,
            ", credentials: ", (function() local c = 0; for _ in pairs(cred_idx) do c = c + 1 end; return c end)())

    return true
end

-- Worker 初始化
local function worker_init_consumers(premature)
    if premature then
        return
    end

    local max_wait = 30
    local waited = 0

    while not store.is_initialized() and waited < max_wait do
        ngx_sleep(0.5)
        waited = waited + 0.5
    end

    if not store.is_initialized() then
        ngx.log(ngx.ERR, "[sys.consumer] store not initialized after ", max_wait, "s")
        return
    end

    local ok = rebuild_consumers()
    if ok then
        ngx.log(ngx.INFO, "[sys.consumer] worker initialized consumers successfully")
    else
        ngx.log(ngx.WARN, "[sys.consumer] worker failed to initialize consumers")
    end
end

-- 协调器
local function coordinator_sync(premature)
    if premature then
        return
    end

    if ngx_process.type() ~= "privileged agent" then
        return
    end

    local check_interval = 3

    while not ngx_worker_exiting() do
        repeat
            if not store.is_initialized() then
                ngx_sleep(1)
                break
            end

            local new_version = store.get_version()

            if new_version ~= current_version then
                local ok, post_err = events.post(
                    _M.events_source_consumer,
                    _M.events_type_rebuild_consumer,
                    { version = new_version }
                )

                if post_err then
                    ngx.log(ngx.WARN, "[sys.consumer] failed to broadcast rebuild signal: ", post_err)
                else
                    ngx.log(ngx.INFO, "[sys.consumer] broadcasted rebuild signal, version: ", new_version)
                    current_version = new_version
                end
            end

            ngx_sleep(check_interval)
        until true
    end
end

-- Worker 事件处理器
local function worker_event_handler_register()
    local rebuild_handler = function(data, event, source)
        if source ~= _M.events_source_consumer or event ~= _M.events_type_rebuild_consumer then
            return
        end

        ngx.log(ngx.INFO, "[sys.consumer] received rebuild signal, version: ", data and data.version or "unknown")
        store.reload()
        rebuild_consumers()
    end

    events.register(rebuild_handler, _M.events_source_consumer, _M.events_type_rebuild_consumer)
end

-- ============================================================
-- 公共 API
-- ============================================================

function _M.init_worker()
    worker_event_handler_register()
    ngx_timer_at(0, worker_init_consumers)

    if ngx_process.type() == "privileged agent" then
        ngx_timer_at(0, coordinator_sync)
    end
end

-- 通过名称获取消费者
function _M.get_by_name(name)
    return consumers[name]
end

-- 通过凭证查找消费者
-- @param cred_type string 凭证类型 (key-auth, basic-auth, jwt-auth 等)
-- @param credential string 凭证值 (API key, username, JWT key 等)
-- @return consumer table 或 nil
function _M.find_by_credential(cred_type, credential)
    if not cred_type or not credential then
        return nil
    end
    
    local key = cred_type .. ":" .. credential
    local consumer_name = credential_index[key]
    
    if not consumer_name then
        return nil
    end
    
    return consumers[consumer_name]
end

-- 验证 API Key
function _M.verify_key_auth(api_key)
    return _M.find_by_credential("key-auth", api_key)
end

-- 验证 Basic Auth
function _M.verify_basic_auth(username)
    local consumer = _M.find_by_credential("basic-auth", username)
    if not consumer then
        return nil
    end
    
    -- 返回消费者及密码供验证
    return consumer, consumer.credentials and consumer.credentials["basic-auth"] and consumer.credentials["basic-auth"].password
end

-- 验证 JWT Key
function _M.verify_jwt_auth(jwt_key)
    local consumer = _M.find_by_credential("jwt-auth", jwt_key)
    if not consumer then
        return nil
    end
    
    -- 返回消费者及 secret
    return consumer, consumer.credentials and consumer.credentials["jwt-auth"] and consumer.credentials["jwt-auth"].secret
end

-- 获取所有消费者 (调试用)
function _M.get_all()
    return consumers
end

return _M
