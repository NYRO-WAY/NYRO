--
-- NYRO System Balancer
--
-- 负载均衡模块，使用 Store 抽象层获取 backends 数据
--

local ngx      = ngx
local ipairs   = ipairs
local pairs    = pairs
local type     = type
local core      = require("nyro.core")
local store    = require("nyro.store")
local events   = require("resty.worker.events")
local ngx_process        = require("ngx.process")
local balancer           = require("ngx.balancer")
local balancer_round     = require('resty.roundrobin')
local balancer_chash     = require('resty.chash')
local ngx_timer_at       = ngx.timer.at
local ngx_sleep          = ngx.sleep
local ngx_worker_exiting = ngx.worker.exiting

local backend_objects = {}
local current_version = 0

local _M = {}

_M.events_source_backend = "events_source_backend"
_M.events_type_rebuild_backend = "events_type_rebuild_backend"

-- 从 Store 获取 backends 数据
local function load_backends_from_store()
    if not store.is_initialized() then
        return nil, "store not initialized"
    end

    local backends, err = store.get_backends()
    if err then
        return nil, err
    end

    if not backends or #backends == 0 then
        return nil, "no backends found"
    end

    return backends
end

-- 规范化 algorithm 名称
local function normalize_algorithm(algo)
    if not algo then
        return core.const.BALANCER_ROUNDROBIN
    end
    
    local upper_algo = string.upper(algo)
    if upper_algo == "ROUNDROBIN" or upper_algo == "ROUND_ROBIN" or upper_algo == "RR" then
        return core.const.BALANCER_ROUNDROBIN
    elseif upper_algo == "CHASH" or upper_algo == "CONSISTENT_HASH" or upper_algo == "CH" then
        return core.const.BALANCER_CHASH
    else
        return core.const.BALANCER_ROUNDROBIN
    end
end

-- 生成 backend balancer 对象
local function generate_backend_balancer(backend_data)
    if not backend_data or type(backend_data) ~= "table" then
        return nil
    end

    local endpoints = backend_data.endpoints
    local endpoint_list = {}
    local endpoint_details = {}  -- key -> 完整 endpoint 信息 (含 headers)

    if endpoints and #endpoints > 0 then
        for idx, endpoint in ipairs(endpoints) do
            local addr = endpoint.address
            local port = endpoint.port
            
            -- 如果 address 包含端口号 (如 "192.168.1.10:9001")
            if addr and not port then
                local host, p = addr:match("^(.+):(%d+)$")
                if host and p then
                    addr = host
                    port = tonumber(p)
                end
            end
            
            if addr and port then
                local weight = endpoint.weight or 1
                -- 使用 idx 保证唯一性 (同 address:port 不同 headers 的场景)
                local key = addr .. '|' .. port .. '|' .. idx
                endpoint_list[key] = weight
                endpoint_details[key] = {
                    address = addr,
                    port = tonumber(port),
                    weight = weight,
                    headers = endpoint.headers,  -- 节点级请求头 (如 API Key)
                }
            end
        end
    end

    if not next(endpoint_list) then
        ngx.log(ngx.WARN, "[sys.balancer] no valid endpoints for backend: ", backend_data.name)
        return nil
    end

    local algorithm = normalize_algorithm(backend_data.algorithm)

    local timeout_config = backend_data.timeout or {}
    local backend_balancer = {
        algorithm        = algorithm,
        read_timeout     = timeout_config.read or core.const.UPSTREAM_DEFAULT_TIMEOUT,
        write_timeout    = timeout_config.send or core.const.UPSTREAM_DEFAULT_TIMEOUT,
        connect_timeout  = timeout_config.connect or core.const.UPSTREAM_DEFAULT_TIMEOUT,
        endpoint_details = endpoint_details,  -- 保留完整 endpoint 信息
    }

    if algorithm == core.const.BALANCER_ROUNDROBIN then
        backend_balancer.handler = balancer_round:new(endpoint_list)
    elseif algorithm == core.const.BALANCER_CHASH then
        backend_balancer.handler = balancer_chash:new(endpoint_list)
    else
        backend_balancer.handler = balancer_round:new(endpoint_list)
    end

    ngx.log(ngx.DEBUG, "[sys.balancer] created balancer for backend: ", backend_data.name, 
            ", algorithm: ", algorithm, ", endpoints: ", (function() local c = 0; for _ in pairs(endpoint_list) do c = c + 1 end; return c end)())

    return backend_balancer
end

-- 重建 backend 对象
local function rebuild_backends()
    local backends, err = load_backends_from_store()
    if err then
        ngx.log(ngx.WARN, "[sys.balancer] failed to load backends: ", err)
        return false
    end

    local new_backend_objects = {}

    for _, backend in ipairs(backends) do
        if backend.name then
            local balancer_obj = generate_backend_balancer(backend)
            if balancer_obj then
                new_backend_objects[backend.name] = balancer_obj
                ngx.log(ngx.DEBUG, "[sys.balancer] loaded backend: ", backend.name)
            end
        end
    end

    backend_objects = new_backend_objects
    current_version = store.get_version()

    ngx.log(ngx.INFO, "[sys.balancer] backends rebuilt, count: ", 
        (function() local c = 0; for _ in pairs(new_backend_objects) do c = c + 1 end; return c end)())

    return true
end

-- Worker 初始化 backends
local function worker_init_backends(premature)
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
        ngx.log(ngx.ERR, "[sys.balancer] store not initialized after ", max_wait, "s")
        return
    end

    local ok = rebuild_backends()
    if ok then
        ngx.log(ngx.INFO, "[sys.balancer] worker initialized backends successfully")
    else
        ngx.log(ngx.WARN, "[sys.balancer] worker failed to initialize backends")
    end
end

-- 协调器：检测配置变更并广播重建信号
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
                    _M.events_source_backend,
                    _M.events_type_rebuild_backend,
                    { version = new_version }
                )

                if post_err then
                    ngx.log(ngx.WARN, "[sys.balancer] failed to broadcast rebuild signal: ", post_err)
                else
                    ngx.log(ngx.INFO, "[sys.balancer] broadcasted rebuild signal, version: ", new_version)
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
        if source ~= _M.events_source_backend or event ~= _M.events_type_rebuild_backend then
            return
        end

        ngx.log(ngx.INFO, "[sys.balancer] received rebuild signal, version: ", data and data.version or "unknown")
        store.reload()
        rebuild_backends()
    end

    events.register(rebuild_handler, _M.events_source_backend, _M.events_type_rebuild_backend)
end

function _M.init_worker()
    worker_event_handler_register()
    ngx_timer_at(0, worker_init_backends)

    if ngx_process.type() == "privileged agent" then
        ngx_timer_at(0, coordinator_sync)
    end
end

-- 检查是否为 IP 地址
local function is_ip_address(addr)
    if not addr then
        return false
    end
    -- IPv4
    if addr:match("^%d+%.%d+%.%d+%.%d+$") then
        return true
    end
    return false
end

-- DNS 解析 (每次创建新的 resolver)
local function resolve_host(host)
    if is_ip_address(host) then
        return host
    end
    
    local resolver = require("resty.dns.resolver")
    local r, err = resolver:new{
        nameservers = { "8.8.8.8", "114.114.114.114" },
        retrans = 3,
        timeout = 2000,
    }
    
    if not r then
        return nil, "resolver init failed: " .. tostring(err)
    end
    
    local answers, query_err = r:query(host, nil, {})
    if not answers then
        return nil, "DNS query failed: " .. tostring(query_err)
    end
    
    for _, ans in ipairs(answers) do
        if ans.type == 1 or ans.type == 28 then -- A or AAAA
            return ans.address
        end
    end
    
    return nil, "no A/AAAA record found"
end

-- ============================================================
-- prepare_upstream: 在 access 阶段完成节点选择 + DNS 解析 + headers 注入
--
-- 设计要点:
--   1. endpoint 选择必须在 access 阶段, 因为 balancer 阶段无法调用
--      ngx.req.set_header (注入 endpoint.headers)
--   2. DNS 解析必须在 access 阶段, 因为 balancer 阶段无法使用 cosocket
--   3. 结果存入 oak_ctx._upstream, balancer 阶段仅执行 set_current_peer
-- ============================================================

function _M.prepare_upstream(oak_ctx)
    if not oak_ctx.config or not oak_ctx.config.service_router or not oak_ctx.config.service_router.router then
        return
    end

    local sr = oak_ctx.config.service_router
    local upstream = sr.router.upstream

    -- 补充 upstream: 如果 router_match 没设, 从 backend_name 获取
    if not upstream or not next(upstream) then
        if oak_ctx.config.backend_name and backend_objects[oak_ctx.config.backend_name] then
            upstream = {
                id = oak_ctx.config.backend_name,
                scheme = oak_ctx.config.service and oak_ctx.config.service.scheme or "http",
            }
            sr.router.upstream = upstream
        else
            return
        end
    end

    -- 获取 service 级 timeout (backend timeout 优先)
    local service = oak_ctx.config.service
    local svc_timeout = service and service.timeout or {}

    if upstream.id then
        -- ── Backend 负载均衡模式 ──────────────────────────────────────────
        local backend_obj = backend_objects[upstream.id]
        if not backend_obj then
            core.log.error("[sys.balancer] backend not found: ", upstream.id)
            return
        end

        -- 选择节点
        local key
        if backend_obj.algorithm == core.const.BALANCER_CHASH then
            key = backend_obj.handler:find(oak_ctx.matched.host or "")
        else
            key = backend_obj.handler:find()
        end

        if not key then
            core.log.error("[sys.balancer] no endpoint selected for backend: ", upstream.id)
            return
        end

        local detail = backend_obj.endpoint_details[key]
        if not detail then
            core.log.error("[sys.balancer] endpoint detail not found: ", key)
            return
        end

        -- DNS 解析
        local address = detail.address
        if not is_ip_address(address) then
            local ip, err = resolve_host(address)
            if ip then
                ngx.log(ngx.INFO, "[sys.balancer] resolved ", address, " -> ", ip)
                address = ip
            else
                core.log.error("[sys.balancer] DNS resolve failed: ", detail.address, " ", err)
                return
            end
        end

        -- 注入 endpoint.headers (如 API Key 轮换)
        if detail.headers and type(detail.headers) == "table" then
            for k, v in pairs(detail.headers) do
                ngx.req.set_header(k, v)
            end
        end

        -- 存入 oak_ctx, 供 balancer 阶段使用
        oak_ctx._upstream = {
            address         = address,
            port            = detail.port,
            host            = detail.address,  -- 原始域名, 用于 Host 头
            scheme          = upstream.scheme or "http",
            connect_timeout = backend_obj.connect_timeout,
            read_timeout    = backend_obj.read_timeout,
            write_timeout   = backend_obj.write_timeout,
        }

    elseif upstream.address then
        -- ── URL 直连模式 ─────────────────────────────────────────────────
        local address = upstream.address
        if not is_ip_address(address) then
            local ip, err = resolve_host(address)
            if ip then
                ngx.log(ngx.INFO, "[sys.balancer] resolved ", address, " -> ", ip)
                address = ip
            else
                core.log.error("[sys.balancer] DNS resolve failed: ", upstream.address, " ", err)
                return
            end
        end

        oak_ctx._upstream = {
            address         = address,
            port            = upstream.port,
            host            = upstream.address,  -- 原始域名
            scheme          = upstream.scheme or "http",
            connect_timeout = svc_timeout.connect or core.const.UPSTREAM_DEFAULT_TIMEOUT,
            read_timeout    = svc_timeout.read or core.const.UPSTREAM_DEFAULT_TIMEOUT,
            write_timeout   = svc_timeout.send or core.const.UPSTREAM_DEFAULT_TIMEOUT,
        }
    end
end

-- 执行负载均衡 (balancer 阶段, 仅设置 peer + timeout)
function _M.gogogo(oak_ctx)
    local up = oak_ctx._upstream
    if not up then
        core.log.error("[sys.balancer] no upstream prepared (call prepare_upstream first)")
        return
    end

    if not up.address or not up.port then
        core.log.error("[sys.balancer] upstream address or port is null")
        return
    end

    -- 设置超时
    local ok, timeout_err = balancer.set_timeouts(
        (up.connect_timeout or core.const.UPSTREAM_DEFAULT_TIMEOUT) / 1000,
        (up.write_timeout or core.const.UPSTREAM_DEFAULT_TIMEOUT) / 1000,
        (up.read_timeout or core.const.UPSTREAM_DEFAULT_TIMEOUT) / 1000
    )
    if not ok then
        core.log.error("[sys.balancer] set timeouts error: ", timeout_err)
    end

    -- 设置目标节点
    local ok2, peer_err = balancer.set_current_peer(up.address, up.port)
    if not ok2 then
        core.log.error("[sys.balancer] set peer error: ", peer_err)
    end
end

-- 获取 backend 对象 (调试用)
function _M.get_backend_objects()
    return backend_objects
end

return _M
