--
-- NYRO System Certificate
--
-- SSL 证书管理模块，使用 Store 抽象层获取证书数据
--

local ngx = ngx
local ipairs = ipairs
local pairs = pairs
local type = type
local io_open = io.open
local core = require("nyro.core")
local store = require("nyro.store")
local events = require("resty.worker.events")
local ngx_ssl = require("ngx.ssl")
local ngx_process = require("ngx.process")
local sys_lru_cache = require("nyro.core.cache")
local ngx_timer_at = ngx.timer.at
local ngx_sleep = ngx.sleep
local ngx_worker_exiting = ngx.worker.exiting

-- 证书索引: { ["example.com"] = { cert = ..., key = ... } }
local ssl_objects = {}
local current_version = 0

local _M = {}

_M.events_source_ssl = "events_source_ssl"
_M.events_type_rebuild_ssl = "events_type_rebuild_ssl"

-- 读取文件内容
local function read_file(path)
    local file, err = io_open(path, "r")
    if not file then
        return nil, "failed to open file: " .. tostring(err)
    end
    local content = file:read("*a")
    file:close()
    return content
end

-- 检查 SNI 是否匹配 (支持通配符)
local function sni_matches(pattern, host)
    if pattern == host then
        return true
    end
    
    -- 通配符匹配: *.example.com
    if pattern:sub(1, 2) == "*." then
        local suffix = pattern:sub(2)  -- .example.com
        if host:sub(-#suffix) == suffix then
            -- 确保只匹配一级子域名
            local prefix = host:sub(1, #host - #suffix)
            if not prefix:find("%.") then
                return true
            end
        end
    end
    
    return false
end

-- 从 Store 加载证书数据
local function load_certificates_from_store()
    if not store.is_initialized() then
        return nil, "store not initialized"
    end

    local certs, err = store.get_certificates()
    if err then
        return nil, err
    end

    return certs or {}
end

-- 重建证书对象
local function rebuild_certificates()
    local certs, err = load_certificates_from_store()
    if err then
        ngx.log(ngx.WARN, "[sys.certificate] failed to load certificates: ", err)
        return false
    end

    local new_ssl_objects = {}

    for _, cert_config in ipairs(certs) do
        repeat
            if not cert_config.name then
                break
            end

            -- 获取证书内容
            local cert_content = cert_config.cert
            local key_content = cert_config.key

            -- 支持从文件读取
            if cert_config.cert_file then
                local content, read_err = read_file(cert_config.cert_file)
                if not content then
                    ngx.log(ngx.WARN, "[sys.certificate] failed to read cert file: ", read_err)
                    break
                end
                cert_content = content
            end

            if cert_config.key_file then
                local content, read_err = read_file(cert_config.key_file)
                if not content then
                    ngx.log(ngx.WARN, "[sys.certificate] failed to read key file: ", read_err)
                    break
                end
                key_content = content
            end

            if not cert_content or not key_content then
                ngx.log(ngx.WARN, "[sys.certificate] missing cert or key for: ", cert_config.name)
                break
            end

            -- 为每个 SNI 创建索引
            local snis = cert_config.snis or {}
            for _, sni in ipairs(snis) do
                new_ssl_objects[sni] = {
                    name = cert_config.name,
                    cert = cert_content,
                    key = key_content,
                }
                ngx.log(ngx.DEBUG, "[sys.certificate] loaded certificate for SNI: ", sni)
            end

        until true
    end

    ssl_objects = new_ssl_objects
    current_version = store.get_version()

    ngx.log(ngx.INFO, "[sys.certificate] certificates rebuilt, count: ", 
        (function() local c = 0; for _ in pairs(new_ssl_objects) do c = c + 1 end; return c end)())

    return true
end

-- Worker 初始化
local function worker_init_certificates(premature)
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
        ngx.log(ngx.ERR, "[sys.certificate] store not initialized after ", max_wait, "s")
        return
    end

    local ok = rebuild_certificates()
    if ok then
        ngx.log(ngx.INFO, "[sys.certificate] worker initialized certificates successfully")
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

    local check_interval = 5

    while not ngx_worker_exiting() do
        repeat
            if not store.is_initialized() then
                ngx_sleep(1)
                break
            end

            local new_version = store.get_version()

            if new_version ~= current_version then
                local ok, post_err = events.post(
                    _M.events_source_ssl,
                    _M.events_type_rebuild_ssl,
                    { version = new_version }
                )

                if post_err then
                    ngx.log(ngx.WARN, "[sys.certificate] failed to broadcast rebuild signal: ", post_err)
                else
                    ngx.log(ngx.INFO, "[sys.certificate] broadcasted rebuild signal, version: ", new_version)
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
        if source ~= _M.events_source_ssl or event ~= _M.events_type_rebuild_ssl then
            return
        end

        ngx.log(ngx.INFO, "[sys.certificate] received rebuild signal, version: ", data and data.version or "unknown")
        store.reload()
        rebuild_certificates()
    end

    events.register(rebuild_handler, _M.events_source_ssl, _M.events_type_rebuild_ssl)
end

-- ============================================================
-- 公共 API
-- ============================================================

function _M.init_worker()
    worker_event_handler_register()
    ngx_timer_at(0, worker_init_certificates)

    if ngx_process.type() == "privileged agent" then
        ngx_timer_at(0, coordinator_sync)
    end
end

-- 获取解析后的证书 (带缓存)
local function fetch_parsed_cert(sni, cert)
    local cache_key = sni .. ":cert"
    local parsed = sys_lru_cache.get(cache_key)

    if not parsed then
        local parsed_cert, err = ngx_ssl.parse_pem_cert(cert)
        if err then
            return nil, err
        end
        sys_lru_cache.set(cache_key, parsed_cert, 3600)
        parsed = parsed_cert
    end

    return parsed, nil
end

-- 获取解析后的私钥 (带缓存)
local function fetch_parsed_priv_key(sni, priv_key)
    local cache_key = sni .. ":key"
    local parsed = sys_lru_cache.get(cache_key)

    if not parsed then
        local parsed_priv_key, err = ngx_ssl.parse_pem_priv_key(priv_key)
        if err then
            return nil, err
        end
        sys_lru_cache.set(cache_key, parsed_priv_key, 3600)
        parsed = parsed_priv_key
    end

    return parsed, nil
end

-- SSL 证书匹配
function _M.ssl_match(oak_ctx)
    if not oak_ctx.matched or not oak_ctx.matched.host then
        core.log.error("ssl_match: oak_ctx data format err: [" .. core.json.encode(oak_ctx, true) .. "]")
        return false
    end

    local host = oak_ctx.matched.host

    -- 查找匹配的证书
    local cert_data = nil
    
    -- 精确匹配
    if ssl_objects[host] then
        cert_data = ssl_objects[host]
    else
        -- 通配符匹配
        for sni, data in pairs(ssl_objects) do
            if sni_matches(sni, host) then
                cert_data = data
                break
            end
        end
    end

    if not cert_data then
        return false
    end

    -- 设置证书
    ngx_ssl.clear_certs()

    local parsed_cert, err = fetch_parsed_cert(host, cert_data.cert)
    if err then
        core.log.error("failed to parse pem cert: ", err)
        return false
    end

    local ok, set_err = ngx_ssl.set_cert(parsed_cert)
    if not ok then
        core.log.error("failed to set pem cert: ", set_err)
        return false
    end

    local parsed_priv_key, key_err = fetch_parsed_priv_key(host, cert_data.key)
    if key_err then
        core.log.error("failed to parse pem priv key: ", key_err)
        return false
    end

    local ok2, set_key_err = ngx_ssl.set_priv_key(parsed_priv_key)
    if not ok2 then
        core.log.error("failed to set pem priv key: ", set_key_err)
        return false
    end

    oak_ctx.config = oak_ctx.config or {}
    oak_ctx.config.cert_key = {
        sni = host,
        name = cert_data.name,
    }

    return true
end

-- 获取所有证书 (调试用)
function _M.get_all()
    return ssl_objects
end

return _M
