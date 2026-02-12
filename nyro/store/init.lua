--
-- NYRO Store 抽象层
-- 
-- 提供统一的数据存储接口，支持多种后端：
-- - standalone: YAML 文件 (DB Less / Admin API 读写)
-- - hybrid: 从 Control Plane 同步 (未来实现)
--

local _M = {
    _VERSION = "3.0.0"
}

-- 存储模式
_M.MODE_STANDALONE = "standalone"
_M.MODE_HYBRID     = "hybrid"

-- 当前适配器实例
local adapter = nil
local current_mode = nil

-- 加载适配器
local function load_adapter(mode)
    if mode == _M.MODE_STANDALONE then
        return require("nyro.store.adapter.yaml")
    elseif mode == _M.MODE_HYBRID then
        return require("nyro.store.adapter.sync")
    else
        return nil, "unknown store mode: " .. tostring(mode)
    end
end

-- 初始化存储层
function _M.init(config)
    if not config then
        return false, "config is required"
    end

    local mode = config.mode or _M.MODE_STANDALONE
    
    local adp, err = load_adapter(mode)
    if not adp then
        return false, err
    end

    local ok, init_err = adp.init(config[mode] or {})
    if not ok then
        return false, init_err
    end

    adapter = adp
    current_mode = mode
    
    return true
end

-- 获取当前模式
function _M.get_mode()
    return current_mode
end

-- 检查是否已初始化
function _M.is_initialized()
    return adapter ~= nil
end

-- ============================================================
-- 资源访问接口 (读)
-- ============================================================

function _M.get_plugins()
    if not adapter then
        return nil, "store not initialized"
    end
    return adapter.get_plugins()
end

function _M.get_backends()
    if not adapter then
        return nil, "store not initialized"
    end
    return adapter.get_backends()
end

function _M.get_services()
    if not adapter then
        return nil, "store not initialized"
    end
    return adapter.get_services()
end

function _M.get_routes()
    if not adapter then
        return nil, "store not initialized"
    end
    return adapter.get_routes()
end

function _M.get_consumers()
    if not adapter then
        return nil, "store not initialized"
    end
    return adapter.get_consumers()
end

function _M.get_certificates()
    if not adapter then
        return nil, "store not initialized"
    end
    return adapter.get_certificates()
end

function _M.get_version()
    if not adapter then
        return nil, "store not initialized"
    end
    return adapter.get_version()
end

-- ============================================================
-- 按 name 查询 (代理到 adapter)
-- ============================================================

function _M.get_route_by_name(name)
    if not adapter or not adapter.get_route_by_name then
        return nil, "store not initialized"
    end
    return adapter.get_route_by_name(name)
end

function _M.get_service_by_name(name)
    if not adapter or not adapter.get_service_by_name then
        return nil, "store not initialized"
    end
    return adapter.get_service_by_name(name)
end

function _M.get_backend_by_name(name)
    if not adapter or not adapter.get_backend_by_name then
        return nil, "store not initialized"
    end
    return adapter.get_backend_by_name(name)
end

function _M.get_consumer_by_name(name)
    if not adapter or not adapter.get_consumer_by_name then
        return nil, "store not initialized"
    end
    return adapter.get_consumer_by_name(name)
end

function _M.get_plugin_by_name(name)
    if not adapter or not adapter.get_plugin_by_name then
        return nil, "store not initialized"
    end
    return adapter.get_plugin_by_name(name)
end

function _M.get_certificate_by_name(name)
    if not adapter or not adapter.get_certificate_by_name then
        return nil, "store not initialized"
    end
    return adapter.get_certificate_by_name(name)
end

-- ============================================================
-- 资源写入接口 (Admin API 使用)
-- ============================================================

local function check_write_support()
    if not adapter then
        return false, "store not initialized"
    end
    return true
end

-- routes
function _M.put_route(name, data)
    local ok, err = check_write_support()
    if not ok then return false, err end
    if not adapter.put_route then return false, "adapter does not support write" end
    return adapter.put_route(name, data)
end

function _M.delete_route(name)
    local ok, err = check_write_support()
    if not ok then return false, err end
    if not adapter.delete_route then return false, "adapter does not support write" end
    return adapter.delete_route(name)
end

-- services
function _M.put_service(name, data)
    local ok, err = check_write_support()
    if not ok then return false, err end
    if not adapter.put_service then return false, "adapter does not support write" end
    return adapter.put_service(name, data)
end

function _M.delete_service(name)
    local ok, err = check_write_support()
    if not ok then return false, err end
    if not adapter.delete_service then return false, "adapter does not support write" end
    return adapter.delete_service(name)
end

-- backends
function _M.put_backend(name, data)
    local ok, err = check_write_support()
    if not ok then return false, err end
    if not adapter.put_backend then return false, "adapter does not support write" end
    return adapter.put_backend(name, data)
end

function _M.delete_backend(name)
    local ok, err = check_write_support()
    if not ok then return false, err end
    if not adapter.delete_backend then return false, "adapter does not support write" end
    return adapter.delete_backend(name)
end

-- consumers
function _M.put_consumer(name, data)
    local ok, err = check_write_support()
    if not ok then return false, err end
    if not adapter.put_consumer then return false, "adapter does not support write" end
    return adapter.put_consumer(name, data)
end

function _M.delete_consumer(name)
    local ok, err = check_write_support()
    if not ok then return false, err end
    if not adapter.delete_consumer then return false, "adapter does not support write" end
    return adapter.delete_consumer(name)
end

-- global plugins
function _M.put_plugin(name, data)
    local ok, err = check_write_support()
    if not ok then return false, err end
    if not adapter.put_plugin then return false, "adapter does not support write" end
    return adapter.put_plugin(name, data)
end

function _M.delete_plugin(name)
    local ok, err = check_write_support()
    if not ok then return false, err end
    if not adapter.delete_plugin then return false, "adapter does not support write" end
    return adapter.delete_plugin(name)
end

-- certificates
function _M.put_certificate(name, data)
    local ok, err = check_write_support()
    if not ok then return false, err end
    if not adapter.put_certificate then return false, "adapter does not support write" end
    return adapter.put_certificate(name, data)
end

function _M.delete_certificate(name)
    local ok, err = check_write_support()
    if not ok then return false, err end
    if not adapter.delete_certificate then return false, "adapter does not support write" end
    return adapter.delete_certificate(name)
end

-- ============================================================
-- 热加载接口
-- ============================================================

function _M.reload()
    if not adapter then
        return false, "store not initialized"
    end
    
    if not adapter.reload then
        return false, "adapter does not support reload"
    end
    
    return adapter.reload()
end

function _M.watch(callback)
    if not adapter then
        return false, "store not initialized"
    end
    
    if not adapter.watch then
        return false, "adapter does not support watch"
    end
    
    return adapter.watch(callback)
end

return _M
