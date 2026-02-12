--
-- NYRO System Router
-- 
-- 路由管理模块，使用 FFI 路由引擎和 Store 抽象层
--

local ngx = ngx
local pairs = pairs
local ipairs = ipairs
local type = type
local core = require("nyro.core")
local store = require("nyro.store")
local events = require("resty.worker.events")
local ngx_process = require("ngx.process")
local ngx_sleep = ngx.sleep
local ngx_timer_at = ngx.timer.at
local ngx_worker_exiting = ngx.worker.exiting

-- 加载路由引擎
local router_matcher = require("nyro.route.matcher")

local router_instance
local current_version = 0

local events_source_router = "events_source_router"
local events_type_rebuild_router = "events_type_rebuild_router"

local _M = {}

-- 从 Store 加载路由数据
local function load_routes_from_store()
    if not store.is_initialized() then
        return nil, "store not initialized"
    end

    local routes, err = store.get_routes()
    if err then
        return nil, err
    end

    local services, _ = store.get_services()
    local backends, _ = store.get_backends()
    local plugins, _ = store.get_plugins()

    -- 构建索引 (使用 name 作为 key)
    local service_map = {}
    if services then
        for _, svc in ipairs(services) do
            if svc.name then
                service_map[svc.name] = svc
            end
        end
    end

    local backend_map = {}
    if backends then
        for _, backend in ipairs(backends) do
            if backend.name then
                backend_map[backend.name] = backend
            end
        end
    end

    local plugin_map = {}
    if plugins then
        for _, plg in ipairs(plugins) do
            if plg.name then
                plugin_map[plg.name] = plg
            end
        end
    end

    return {
        routes = routes or {},
        services = service_map,
        backends = backend_map,
        plugins = plugin_map,
    }
end

-- 解析 URL 获取 host/port/path
local function parse_url(url)
    if not url then
        return nil
    end
    
    -- 格式: scheme://host:port/path
    local scheme, host, port, path = url:match("^(https?)://([^:/]+):?(%d*)(.*)$")
    if not scheme then
        return nil
    end
    
    port = tonumber(port) or (scheme == "https" and 443 or 80)
    path = path ~= "" and path or "/"
    
    return {
        scheme = scheme,
        host = host,
        port = port,
        path = path,
    }
end

-- 构建路由表
local function build_router(data)
    local r, err = router_matcher.new()
    if not r then
        return nil, "failed to create router: " .. tostring(err)
    end

    local routes = data.routes or {}
    local services = data.services or {}

    local added_count = 0
    for _, route in ipairs(routes) do
        local paths = route.paths or {}
        local methods = route.methods or {"GET", "POST", "PUT", "DELETE", "PATCH", "HEAD", "OPTIONS"}
        local hosts = route.hosts  -- 域名数组, nil 表示匹配所有

        -- 获取关联的服务
        local service = nil
        if route.service then
            service = services[route.service]
        end

        -- 确定后端: backend 或 url 直接代理
        local backend_name = nil
        local upstream_url = nil
        
        if service then
            if service.backend then
                backend_name = service.backend
            elseif service.url then
                upstream_url = parse_url(service.url)
            end
        end

        -- 获取匹配类型
        local match_type = route.match_type

        -- 构建 handler (存储完整 service 对象, 供 ai-proxy 等插件读取 provider)
        local handler = {
            route = route,
            service = service,
            backend_name = backend_name,
            upstream_url = upstream_url,
            plugins = route.plugins or {},
        }

        -- hosts 为空时用 {"*"} 匹配所有域名
        local host_list = hosts
        if not host_list or #host_list == 0 then
            host_list = {"*"}
        end

        for _, host in ipairs(host_list) do
            for _, path in ipairs(paths) do
                local ok, add_err = r:add({
                    host = host,
                    path = path,
                    methods = methods,
                    match_type = match_type,
                    priority = route.priority or 0,
                    handler = handler,
                })

                if ok then
                    added_count = added_count + 1
                else
                    ngx.log(ngx.WARN, "[sys.router] failed to add route: ", route.name,
                            " host: ", host, " path: ", path, " err: ", add_err)
                end
            end
        end
    end

    if added_count == 0 then
        ngx.log(ngx.WARN, "[sys.router] no routes added to router")
        return nil, "no routes added"
    end

    local ok, build_err = r:build()
    if not ok then
        return nil, "failed to build router: " .. tostring(build_err)
    end

    ngx.log(ngx.INFO, "[sys.router] router built with ", r:count(), " routes")
    return r
end

-- 重新构建路由表
local function rebuild_router()
    local data, err = load_routes_from_store()
    if err then
        ngx.log(ngx.ERR, "[sys.router] failed to load routes from store: ", err)
        return false
    end

    if not data.routes or #data.routes == 0 then
        ngx.log(ngx.WARN, "[sys.router] no routes loaded from store")
        return false
    end

    local new_router, build_err = build_router(data)
    if new_router then
        router_instance = new_router
        current_version = store.get_version()
        ngx.log(ngx.INFO, "[sys.router] router updated, version: ", current_version)
        return true
    else
        ngx.log(ngx.ERR, "[sys.router] failed to build router: ", build_err)
        return false
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

    local check_interval = 2

    while not ngx_worker_exiting() do
        repeat
            if not store.is_initialized() then
                ngx_sleep(1)
                break
            end

            local new_version = store.get_version()

            if new_version ~= current_version then
                local ok, post_err = events.post(
                    events_source_router, 
                    events_type_rebuild_router, 
                    { version = new_version }
                )

                if post_err then
                    ngx.log(ngx.WARN, "[sys.router] failed to broadcast rebuild signal: ", post_err)
                else
                    ngx.log(ngx.INFO, "[sys.router] broadcasted rebuild signal, version: ", new_version)
                    current_version = new_version
                end
            end

            ngx_sleep(check_interval)
        until true
    end
end

-- Worker 初始化路由表
local function worker_init_router(premature)
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
        ngx.log(ngx.ERR, "[sys.router] store not initialized after ", max_wait, "s")
        return
    end

    local ok = rebuild_router()
    if ok then
        ngx.log(ngx.INFO, "[sys.router] worker initialized router successfully")
    else
        ngx.log(ngx.WARN, "[sys.router] worker failed to initialize router")
    end
end

-- Worker 事件处理器
local function worker_event_handler_register()
    local rebuild_handler = function(data, event, source)
        if source ~= events_source_router or event ~= events_type_rebuild_router then
            return
        end

        ngx.log(ngx.INFO, "[sys.router] received rebuild signal, version: ", data and data.version or "unknown")
        -- 先从磁盘刷新内存, 确保拿到最新配置
        store.reload()
        rebuild_router()
    end

    events.register(rebuild_handler, events_source_router, events_type_rebuild_router)
end

function _M.init_worker()
    worker_event_handler_register()
    ngx_timer_at(0, worker_init_router)
    
    if ngx_process.type() == "privileged agent" then
        ngx_timer_at(0, coordinator_sync)
    end
end

-- 提取请求参数
function _M.parameter(oak_ctx)
    local env = core.request.header(core.const.REQUEST_API_ENV_KEY)
    if env then
        env = core.string.upper(env)
    else
        env = core.const.ENVIRONMENT_PROD
    end

    oak_ctx.matched = {}
    oak_ctx.matched.host = ngx.var.host
    oak_ctx.matched.uri = ngx.var.uri
    oak_ctx.matched.scheme = ngx.var.scheme
    oak_ctx.matched.query = core.request.query()
    oak_ctx.matched.method = core.request.get_method()
    oak_ctx.matched.header = core.request.header()
    oak_ctx.matched.header[core.const.REQUEST_API_ENV_KEY] = env
end

-- 路由匹配
function _M.router_match(oak_ctx)
    if not oak_ctx.matched or not oak_ctx.matched.uri then
        core.log.error("[sys.router] oak_ctx data format error")
        return false
    end

    if not router_instance then
        core.log.error("[sys.router] router not initialized")
        return false
    end

    local handler, params, match_type = router_instance:match(
        oak_ctx.matched.host,
        oak_ctx.matched.uri,
        oak_ctx.matched.method
    )

    if not handler then
        return false
    end

    -- 设置匹配结果
    oak_ctx.matched.path = params or {}
    oak_ctx.config = {
        route = handler.route,
        service = handler.service,
        backend_name = handler.backend_name,
        upstream_url = handler.upstream_url,
        plugins = handler.plugins,
        match_type = match_type,
    }

    -- 构建 service_router 结构 (用于 balancer)
    local upstream = nil
    if handler.backend_name then
        upstream = {
            id = handler.backend_name,
            -- backend 模式: scheme 从 service.scheme 获取 (默认 http)
            scheme = handler.service and handler.service.scheme or "http",
        }
    elseif handler.upstream_url then
        upstream = {
            address = handler.upstream_url.host,
            port = handler.upstream_url.port,
            scheme = handler.upstream_url.scheme,
            path = handler.upstream_url.path,
        }
    end

    oak_ctx.config.service_router = {
        protocols = handler.service and handler.service.protocol and {handler.service.protocol} or {"http"},
        plugins = handler.service and handler.service.plugins or {},
        router = {
            path = oak_ctx.matched.uri,
            plugins = handler.plugins or {},
            upstream = upstream,
            headers = handler.route and handler.route.headers or {},
            methods = handler.route and handler.route.methods or {},
        }
    }

    return true
end

return _M
