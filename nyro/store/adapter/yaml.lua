--
-- NYRO YAML Adapter
-- 
-- DB Less 模式的存储适配器，从 YAML 文件加载配置
-- 支持读取和写入 (Admin API 使用)
--

local yaml = require("tinyyaml")
local io_open = io.open
local ngx = ngx
local type = type
local pairs = pairs
local ipairs = ipairs
local table_insert = table.insert
local table_remove = table.remove

local _M = {
    _VERSION = "3.0.0"
}

-- 内部状态
local config_data = nil
local config_version = 0
local config_file_path = nil
local watchers = {}

-- 默认配置文件路径
local DEFAULT_CONFIG_FILE = "conf/config.yaml"

-- 资源类型 → config_data 中的 key
local RESOURCE_KEYS = {
    route       = "routes",
    service     = "services",
    backend     = "backends",
    consumer    = "consumers",
    plugin      = "plugins",
    certificate = "certificates",
}

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

-- 解析 YAML 内容
local function parse_yaml(content)
    if not content or content == "" then
        return nil, "empty content"
    end
    
    local ok, data = pcall(yaml.parse, content)
    if not ok then
        return nil, "YAML parse error: " .. tostring(data)
    end
    
    return data
end

-- 验证配置结构
local function validate_config(config)
    if type(config) ~= "table" then
        return false, "config must be a table"
    end
    
    local valid_sections = {
        version = true,
        plugins = true,
        backends = true,
        services = true,
        routes = true,
        consumers = true,
        certificates = true,
    }
    
    for key, _ in pairs(config) do
        if not valid_sections[key] then
            ngx.log(ngx.WARN, "[store.yaml] unknown config section: ", key)
        end
    end
    
    return true
end

-- 构建索引 (使用 name 作为 key)
local function build_index_by_name(list)
    if not list then
        return {}
    end
    local index = {}
    for _, item in ipairs(list) do
        if item.name then
            index[item.name] = item
        end
    end
    return index
end

-- 重建所有索引
local function rebuild_all_index(data)
    data._index = {
        plugins = build_index_by_name(data.plugins),
        backends = build_index_by_name(data.backends),
        services = build_index_by_name(data.services),
        routes = build_index_by_name(data.routes),
        consumers = build_index_by_name(data.consumers),
        certificates = build_index_by_name(data.certificates),
    }
end

-- 跨进程版本共享: 使用 lua_shared_dict 让所有 worker + privileged agent 看到同一个版本号
local SHARED_DICT_NAME = "nyro_config_version"
local SHARED_VERSION_KEY = "version"

local function shared_version_get()
    local dict = ngx.shared[SHARED_DICT_NAME]
    if dict then
        return dict:get(SHARED_VERSION_KEY) or 0
    end
    return config_version
end

local function shared_version_incr()
    local dict = ngx.shared[SHARED_DICT_NAME]
    if dict then
        local new_ver, err = dict:incr(SHARED_VERSION_KEY, 1, 0)
        if new_ver then
            config_version = new_ver
            return new_ver
        end
        ngx.log(ngx.ERR, "[store.yaml] shared dict incr failed: ", err)
    end
    config_version = config_version + 1
    return config_version
end

-- 加载配置文件 (不递增共享版本, 仅同步本地变量)
local function load_config()
    if not config_file_path then
        return false, "config file path not set"
    end
    
    -- 读取文件
    local content, err = read_file(config_file_path)
    if not content then
        return false, err
    end
    
    -- 解析 YAML
    local data, parse_err = parse_yaml(content)
    if not data then
        return false, parse_err
    end
    
    -- 验证配置
    local valid, valid_err = validate_config(data)
    if not valid then
        return false, valid_err
    end
    
    -- 构建索引
    rebuild_all_index(data)
    
    -- 同步本地版本号 (从 shared dict 读取, 不递增)
    config_version = shared_version_get()
    config_data = data
    
    ngx.log(ngx.INFO, "[store.yaml] config loaded, version: ", config_version, 
            ", backends: ", #(data.backends or {}),
            ", services: ", #(data.services or {}),
            ", routes: ", #(data.routes or {}))
    
    return true
end

-- 通知所有监听者
local function notify_watchers(event_type, data)
    for _, callback in ipairs(watchers) do
        local ok, err = pcall(callback, event_type, data)
        if not ok then
            ngx.log(ngx.ERR, "[store.yaml] watcher callback error: ", err)
        end
    end
end

-- ============================================================
-- YAML 序列化 (简易但足够的 Lua table → YAML string)
-- ============================================================

local serialize_value  -- forward declaration

local function indent(level)
    return string.rep("  ", level)
end

local function serialize_list(list, level)
    local lines = {}
    for _, item in ipairs(list) do
        if type(item) == "table" then
            -- 检查是否为简单 KV 对象 (非嵌套)
            local first = true
            for k, v in pairs(item) do
                if k ~= "_index" then
                    if first then
                        lines[#lines + 1] = indent(level) .. "- " .. tostring(k) .. ": " .. serialize_value(v, level + 2, true)
                        first = false
                    else
                        lines[#lines + 1] = indent(level + 1) .. tostring(k) .. ": " .. serialize_value(v, level + 2, true)
                    end
                end
            end
        else
            lines[#lines + 1] = indent(level) .. "- " .. serialize_value(item, level + 1, false)
        end
    end
    return table.concat(lines, "\n")
end

serialize_value = function(val, level, inline_first)
    if val == nil then
        return "null"
    elseif type(val) == "boolean" then
        return val and "true" or "false"
    elseif type(val) == "number" then
        return tostring(val)
    elseif type(val) == "string" then
        -- 需要引号的场景: 包含特殊字符或看起来像数字/布尔
        if val == "" or val:match("^%s") or val:match("%s$")
            or val:match("[:#{}%[%],&*?|>!%%@`]")
            or val == "true" or val == "false"
            or val == "null" or val == "yes" or val == "no"
            or tonumber(val) then
            return '"' .. val:gsub('\\', '\\\\'):gsub('"', '\\"') .. '"'
        end
        return val
    elseif type(val) == "table" then
        -- 检查是否为数组
        if #val > 0 or next(val) == nil then
            -- 短数组使用 flow style: ["a", "b"]
            local all_scalar = true
            for _, v in ipairs(val) do
                if type(v) == "table" then
                    all_scalar = false
                    break
                end
            end
            if all_scalar and #val <= 10 then
                local items = {}
                for _, v in ipairs(val) do
                    items[#items + 1] = serialize_value(v, level, false)
                end
                return "[" .. table.concat(items, ", ") .. "]"
            end
            -- 长/复杂数组使用 block style
            return "\n" .. serialize_list(val, level)
        else
            -- 对象
            local lines = {}
            for k, v in pairs(val) do
                if k ~= "_index" then
                    lines[#lines + 1] = indent(level) .. tostring(k) .. ": " .. serialize_value(v, level + 1, true)
                end
            end
            return "\n" .. table.concat(lines, "\n")
        end
    end
    return tostring(val)
end

local function serialize_yaml(data)
    local lines = {}
    local version = data.version or "1.0"
    lines[#lines + 1] = 'version: "' .. tostring(version) .. '"'
    lines[#lines + 1] = ""

    -- 按固定顺序输出资源段
    local section_order = { "consumers", "services", "backends", "routes", "plugins", "certificates" }

    for _, section in ipairs(section_order) do
        local items = data[section]
        if items and type(items) == "table" and #items > 0 then
            lines[#lines + 1] = section .. ":"
            lines[#lines + 1] = serialize_list(items, 1)
            lines[#lines + 1] = ""
        end
    end

    return table.concat(lines, "\n") .. "\n"
end

-- ============================================================
-- 原子写入配置文件
-- ============================================================

local function write_config_file()
    if not config_file_path or not config_data then
        return false, "config file path or data not set"
    end

    -- 序列化 (排除 _index)
    local content = serialize_yaml(config_data)

    -- 原子写入: 先写 .tmp 再 rename
    local tmp_path = config_file_path .. ".tmp"
    local bak_path = config_file_path .. ".bak"

    -- 写入临时文件
    local f, err = io_open(tmp_path, "w")
    if not f then
        return false, "failed to open tmp file: " .. tostring(err)
    end
    f:write(content)
    f:close()

    -- 备份当前文件
    os.rename(config_file_path, bak_path)

    -- 原子替换
    local ok, rename_err = os.rename(tmp_path, config_file_path)
    if not ok then
        -- 回滚
        os.rename(bak_path, config_file_path)
        return false, "failed to rename tmp file: " .. tostring(rename_err)
    end

    -- 写入成功，清理备份文件
    os.remove(bak_path)

    ngx.log(ngx.INFO, "[store.yaml] config file written, version: ", config_version)
    return true
end

-- ============================================================
-- 引用校验
-- ============================================================

local function check_service_exists(service_name)
    if not service_name then
        return true  -- service 为空时不校验
    end
    if not config_data or not config_data._index then
        return false
    end
    return config_data._index.services[service_name] ~= nil
end

local function check_backend_exists(backend_name)
    if not backend_name then
        return true
    end
    if not config_data or not config_data._index then
        return false
    end
    return config_data._index.backends[backend_name] ~= nil
end

-- 检查 service 是否被任何 route 引用
local function is_service_referenced(service_name)
    local routes = config_data.routes or {}
    for _, r in ipairs(routes) do
        if r.service == service_name then
            return true, r.name
        end
    end
    return false
end

-- 检查 backend 是否被任何 service 引用
local function is_backend_referenced(backend_name)
    local services = config_data.services or {}
    for _, s in ipairs(services) do
        if s.backend == backend_name then
            return true, s.name
        end
    end
    return false
end

-- ============================================================
-- 通用写入辅助
-- ============================================================

-- 在列表中找到 name 匹配的项并返回 index
local function find_index_by_name(list, name)
    if not list then
        return nil
    end
    for i, item in ipairs(list) do
        if item.name == name then
            return i
        end
    end
    return nil
end

-- 递增版本并触发通知 (写入 shared dict, 跨进程可见)
local function bump_version()
    shared_version_incr()
    rebuild_all_index(config_data)
    notify_watchers("update", { version = config_version })
end

-- 写入前刷新: 从文件重新加载以获取最新状态 (跨 worker 一致性)
local function ensure_fresh()
    local content, err = read_file(config_file_path)
    if not content then
        return false, err
    end

    local data, parse_err = parse_yaml(content)
    if not data then
        return false, parse_err
    end

    rebuild_all_index(data)
    config_data = data

    return true
end

-- ============================================================
-- 公共 API
-- ============================================================

-- 初始化适配器
function _M.init(config)
    config = config or {}
    
    if config.config_file then
        config_file_path = config.config_file
    else
        local prefix = ngx.config.prefix()
        config_file_path = prefix .. DEFAULT_CONFIG_FILE
    end
    
    ngx.log(ngx.INFO, "[store.yaml] initializing with config file: ", config_file_path)
    
    local ok, err = load_config()
    if not ok then
        return false, err
    end
    
    return true
end

-- 重新加载配置
function _M.reload()
    local old_version = config_version
    
    local ok, err = load_config()
    if not ok then
        return false, err
    end
    
    if config_version > old_version then
        notify_watchers("reload", {
            old_version = old_version,
            new_version = config_version,
        })
    end
    
    return true
end

-- 监听配置变更
function _M.watch(callback)
    if type(callback) ~= "function" then
        return false, "callback must be a function"
    end
    
    table_insert(watchers, callback)
    return true
end

-- 获取配置版本号 (从 shared dict 读取, 跨进程一致)
function _M.get_version()
    return shared_version_get()
end

-- ============================================================
-- 资源访问接口 (读)
-- ============================================================

function _M.get_plugins()
    if not config_data then
        return nil, "config not loaded"
    end
    return config_data.plugins or {}, nil
end

function _M.get_backends()
    if not config_data then
        return nil, "config not loaded"
    end
    return config_data.backends or {}, nil
end

function _M.get_services()
    if not config_data then
        return nil, "config not loaded"
    end
    return config_data.services or {}, nil
end

function _M.get_routes()
    if not config_data then
        return nil, "config not loaded"
    end
    return config_data.routes or {}, nil
end

function _M.get_consumers()
    if not config_data then
        return nil, "config not loaded"
    end
    return config_data.consumers or {}, nil
end

function _M.get_certificates()
    if not config_data then
        return nil, "config not loaded"
    end
    return config_data.certificates or {}, nil
end

-- ============================================================
-- 按 name 查询接口
-- ============================================================

function _M.get_plugin_by_name(name)
    if not config_data or not config_data._index then
        return nil, "config not loaded"
    end
    return config_data._index.plugins[name]
end

function _M.get_backend_by_name(name)
    if not config_data or not config_data._index then
        return nil, "config not loaded"
    end
    return config_data._index.backends[name]
end

function _M.get_service_by_name(name)
    if not config_data or not config_data._index then
        return nil, "config not loaded"
    end
    return config_data._index.services[name]
end

function _M.get_route_by_name(name)
    if not config_data or not config_data._index then
        return nil, "config not loaded"
    end
    return config_data._index.routes[name]
end

function _M.get_consumer_by_name(name)
    if not config_data or not config_data._index then
        return nil, "config not loaded"
    end
    return config_data._index.consumers[name]
end

function _M.get_certificate_by_name(name)
    if not config_data or not config_data._index then
        return nil, "config not loaded"
    end
    return config_data._index.certificates[name]
end

-- ============================================================
-- 资源写入接口
-- ============================================================

-- PUT route (create or replace)
function _M.put_route(name, data)
    if not config_data then
        return false, "config not loaded"
    end
    if not name or name == "" then
        return false, "name is required"
    end

    -- 跨 worker 一致性: 从文件刷新内存
    ensure_fresh()

    -- 引用校验: service 必须存在
    if data.service and not check_service_exists(data.service) then
        return false, "referenced service not found: " .. data.service
    end

    data.name = name
    config_data.routes = config_data.routes or {}

    local idx = find_index_by_name(config_data.routes, name)
    if idx then
        config_data.routes[idx] = data
    else
        table_insert(config_data.routes, data)
    end

    bump_version()

    local ok, err = write_config_file()
    if not ok then
        return false, "persisted to memory but failed to write file: " .. tostring(err)
    end

    return true
end

-- DELETE route
function _M.delete_route(name)
    if not config_data then
        return false, "config not loaded"
    end

    ensure_fresh()
    config_data.routes = config_data.routes or {}
    local idx = find_index_by_name(config_data.routes, name)
    if not idx then
        return false, "route not found: " .. name
    end

    table_remove(config_data.routes, idx)
    bump_version()

    local ok, err = write_config_file()
    if not ok then
        return false, "removed from memory but failed to write file: " .. tostring(err)
    end

    return true
end

-- PUT service
function _M.put_service(name, data)
    if not config_data then
        return false, "config not loaded"
    end
    if not name or name == "" then
        return false, "name is required"
    end

    ensure_fresh()

    -- 引用校验: backend 必须存在
    if data.backend and not check_backend_exists(data.backend) then
        return false, "referenced backend not found: " .. data.backend
    end

    data.name = name
    config_data.services = config_data.services or {}

    local idx = find_index_by_name(config_data.services, name)
    if idx then
        config_data.services[idx] = data
    else
        table_insert(config_data.services, data)
    end

    bump_version()

    local ok, err = write_config_file()
    if not ok then
        return false, "persisted to memory but failed to write file: " .. tostring(err)
    end

    return true
end

-- DELETE service
function _M.delete_service(name)
    if not config_data then
        return false, "config not loaded"
    end

    ensure_fresh()

    -- 引用校验: 不能删除正被 route 引用的 service
    local referenced, by_route = is_service_referenced(name)
    if referenced then
        return false, "service is referenced by route: " .. tostring(by_route)
    end

    config_data.services = config_data.services or {}
    local idx = find_index_by_name(config_data.services, name)
    if not idx then
        return false, "service not found: " .. name
    end

    table_remove(config_data.services, idx)
    bump_version()

    local ok, err = write_config_file()
    if not ok then
        return false, "removed from memory but failed to write file: " .. tostring(err)
    end

    return true
end

-- PUT backend
function _M.put_backend(name, data)
    if not config_data then
        return false, "config not loaded"
    end
    if not name or name == "" then
        return false, "name is required"
    end

    ensure_fresh()
    data.name = name
    config_data.backends = config_data.backends or {}

    local idx = find_index_by_name(config_data.backends, name)
    if idx then
        config_data.backends[idx] = data
    else
        table_insert(config_data.backends, data)
    end

    bump_version()

    local ok, err = write_config_file()
    if not ok then
        return false, "persisted to memory but failed to write file: " .. tostring(err)
    end

    return true
end

-- DELETE backend
function _M.delete_backend(name)
    if not config_data then
        return false, "config not loaded"
    end

    ensure_fresh()

    -- 引用校验: 不能删除正被 service 引用的 backend
    local referenced, by_svc = is_backend_referenced(name)
    if referenced then
        return false, "backend is referenced by service: " .. tostring(by_svc)
    end

    config_data.backends = config_data.backends or {}
    local idx = find_index_by_name(config_data.backends, name)
    if not idx then
        return false, "backend not found: " .. name
    end

    table_remove(config_data.backends, idx)
    bump_version()

    local ok, err = write_config_file()
    if not ok then
        return false, "removed from memory but failed to write file: " .. tostring(err)
    end

    return true
end

-- PUT consumer
function _M.put_consumer(name, data)
    if not config_data then
        return false, "config not loaded"
    end
    if not name or name == "" then
        return false, "name is required"
    end

    ensure_fresh()
    data.name = name
    config_data.consumers = config_data.consumers or {}

    local idx = find_index_by_name(config_data.consumers, name)
    if idx then
        config_data.consumers[idx] = data
    else
        table_insert(config_data.consumers, data)
    end

    bump_version()

    local ok, err = write_config_file()
    if not ok then
        return false, "persisted to memory but failed to write file: " .. tostring(err)
    end

    return true
end

-- DELETE consumer
function _M.delete_consumer(name)
    if not config_data then
        return false, "config not loaded"
    end

    ensure_fresh()
    config_data.consumers = config_data.consumers or {}
    local idx = find_index_by_name(config_data.consumers, name)
    if not idx then
        return false, "consumer not found: " .. name
    end

    table_remove(config_data.consumers, idx)
    bump_version()

    local ok, err = write_config_file()
    if not ok then
        return false, "removed from memory but failed to write file: " .. tostring(err)
    end

    return true
end

-- PUT global plugin config
function _M.put_plugin(name, data)
    if not config_data then
        return false, "config not loaded"
    end
    if not name or name == "" then
        return false, "name is required"
    end

    ensure_fresh()
    data.name = name
    config_data.plugins = config_data.plugins or {}

    local idx = find_index_by_name(config_data.plugins, name)
    if idx then
        config_data.plugins[idx] = data
    else
        table_insert(config_data.plugins, data)
    end

    bump_version()

    local ok, err = write_config_file()
    if not ok then
        return false, "persisted to memory but failed to write file: " .. tostring(err)
    end

    return true
end

-- DELETE global plugin config
function _M.delete_plugin(name)
    if not config_data then
        return false, "config not loaded"
    end

    ensure_fresh()
    config_data.plugins = config_data.plugins or {}
    local idx = find_index_by_name(config_data.plugins, name)
    if not idx then
        return false, "plugin not found: " .. name
    end

    table_remove(config_data.plugins, idx)
    bump_version()

    local ok, err = write_config_file()
    if not ok then
        return false, "removed from memory but failed to write file: " .. tostring(err)
    end

    return true
end

-- PUT certificate
function _M.put_certificate(name, data)
    if not config_data then
        return false, "config not loaded"
    end
    if not name or name == "" then
        return false, "name is required"
    end

    ensure_fresh()
    data.name = name
    config_data.certificates = config_data.certificates or {}

    local idx = find_index_by_name(config_data.certificates, name)
    if idx then
        config_data.certificates[idx] = data
    else
        table_insert(config_data.certificates, data)
    end

    bump_version()

    local ok, err = write_config_file()
    if not ok then
        return false, "persisted to memory but failed to write file: " .. tostring(err)
    end

    return true
end

-- DELETE certificate
function _M.delete_certificate(name)
    if not config_data then
        return false, "config not loaded"
    end

    ensure_fresh()
    config_data.certificates = config_data.certificates or {}
    local idx = find_index_by_name(config_data.certificates, name)
    if not idx then
        return false, "certificate not found: " .. name
    end

    table_remove(config_data.certificates, idx)
    bump_version()

    local ok, err = write_config_file()
    if not ok then
        return false, "removed from memory but failed to write file: " .. tostring(err)
    end

    return true
end

return _M
