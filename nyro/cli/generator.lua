local common = require("nyro/cli/utils/common")

local _M = {}

--- 安全获取嵌套配置值，支持点号路径 (如 "nginx.worker_processes")
local function get(tbl, path, default)
    local current = tbl
    for key in path:gmatch("[^%.]+") do
        if type(current) ~= "table" then
            return default
        end
        current = current[key]
        if current == nil then
            return default
        end
    end
    return current
end

--- 模板渲染: 将 {{ key }} 替换为 context[key]
local function render(template, context)
    return (template:gsub("{{%s*(.-)%s*}}", function(key)
        local value = context[key]
        if value == nil then
            error("template variable not found: " .. key)
        end
        return tostring(value)
    end))
end

-- ============================================================
-- 环境检测
-- ============================================================

--- 从 openresty -V 解析 --conf-path，推导 mime.types 绝对路径
local function detect_mime_types_path()
    local output = common.execute_cmd("openresty -V 2>&1")

    -- 优先从 --conf-path 推导（最准确）
    local conf_path = output:match("%-%-conf%-path=([^%s]+)")
    if conf_path then
        local conf_dir = conf_path:match("(.+)/[^/]+$")
        if conf_dir then
            local path = conf_dir .. "/mime.types"
            local f = io.open(path, "r")
            if f then
                f:close()
                return path
            end
        end
    end

    -- 备选：从 --prefix 推导
    local prefix = output:match("%-%-prefix=([^%s]+)")
    if prefix then
        local path = prefix .. "/conf/mime.types"
        local f = io.open(path, "r")
        if f then
            f:close()
            return path
        end
    end

    -- 兜底：常见安装路径
    local fallbacks = {
        "/usr/local/openresty/nginx/conf/mime.types",
        "/opt/homebrew/etc/openresty/mime.types",
        "/usr/local/opt/openresty/nginx/conf/mime.types",
    }
    for _, path in ipairs(fallbacks) do
        local f = io.open(path, "r")
        if f then
            f:close()
            return path
        end
    end

    return nil
end

--- 确保占位 SSL 证书存在，不存在则用 openssl 自动生成
local function ensure_placeholder_ssl(home)
    local ssl_dir   = home .. "/ssl"
    local cert_path = ssl_dir .. "/placeholder.crt"
    local key_path  = ssl_dir .. "/placeholder.key"

    -- 已存在则跳过
    local f = io.open(cert_path, "r")
    if f then
        f:close()
        return cert_path, key_path
    end

    os.execute("mkdir -p " .. ssl_dir)

    local cmd = string.format(
        'openssl req -x509 -nodes -days 36500 -newkey rsa:2048 '
        .. '-keyout %s -out %s -subj "/CN=NYRO" 2>/dev/null',
        key_path, cert_path
    )
    -- os.execute 返回值兼容: LuaJIT 2.1+ 返回 true, Lua 5.1 返回 0
    local ret = os.execute(cmd)
    if ret ~= true and ret ~= 0 then
        print("Generate SSL Cert      ...FAIL (openssl command failed)")
        os.exit(1)
    end

    print("Generate SSL Cert      ...OK (self-signed placeholder)")

    return cert_path, key_path
end

-- ============================================================
-- 模板块构建
-- ============================================================

--- 构建 resolver 指令
local function build_resolver(config)
    local resolvers = get(config, "nginx.resolver", { "8.8.8.8", "114.114.114.114" })
    local ipv6 = get(config, "nginx.resolver_ipv6", false)

    local parts = {}
    for _, r in ipairs(resolvers) do
        parts[#parts + 1] = r
    end

    if not ipv6 then
        parts[#parts + 1] = "ipv6=off"
    end

    return "    resolver " .. table.concat(parts, " ") .. ";"
end

--- 构建 lua_shared_dict 指令块 (按 key 排序保证输出稳定)
local function build_shared_dicts(dict_config)
    if not dict_config or type(dict_config) ~= "table" then
        return ""
    end

    local keys = {}
    for k in pairs(dict_config) do
        keys[#keys + 1] = k
    end
    table.sort(keys)

    local lines = {}
    for _, name in ipairs(keys) do
        lines[#lines + 1] = string.format("    lua_shared_dict %s %s;", name, dict_config[name])
    end

    return table.concat(lines, "\n")
end

--- 构建 listen 指令
local function build_listen(ports, suffix)
    suffix = suffix or ""
    local lines = {}
    for _, port in ipairs(ports) do
        if suffix ~= "" then
            lines[#lines + 1] = string.format("        listen %s %s;", tostring(port), suffix)
        else
            lines[#lines + 1] = string.format("        listen %s;", tostring(port))
        end
    end
    return table.concat(lines, "\n")
end

--- 构建 Admin Server 块 (admin.enabled=false 时返回空字符串)
local function build_admin_server(config)
    local enabled = get(config, "admin.enabled", false)
    if not enabled then
        return ""
    end

    local listen_ports = get(config, "admin.listen", { 11080, 11443 })
    if type(listen_ports) == "string" then
        listen_ports = { listen_ports }
    end

    local listen_lines = {}
    for _, port in ipairs(listen_ports) do
        listen_lines[#listen_lines + 1] = string.format("        listen %s;", tostring(port))
    end

    return table.concat({
        "    server {",
        table.concat(listen_lines, "\n"),
        "",
        "        location /nyro/admin {",
        "            content_by_lua_block {",
        "                nyro.http_admin()",
        "            }",
        "        }",
        "",
        "        location /nyro/console {",
        "            index index.html;",
        "            alias console/;",
        "",
        "            try_files $uri $uri/ /index.html;",
        "        }",
        "    }",
    }, "\n")
end

--- 从 nyro.yaml 配置构建模板上下文
local function build_context(config, home)
    local ctx = {}

    -- Nginx 核心参数
    ctx.worker_processes        = get(config, "nginx.worker_processes",        "auto")
    ctx.worker_connections      = get(config, "nginx.worker_connections",      10620)
    ctx.worker_rlimit_nofile    = get(config, "nginx.worker_rlimit_nofile",   20480)
    ctx.worker_shutdown_timeout = get(config, "nginx.worker_shutdown_timeout", 3)
    ctx.error_log               = get(config, "nginx.error_log",              "logs/error.log")
    ctx.error_log_level         = get(config, "logging.level",                "error")
    ctx.access_log              = get(config, "nginx.access_log",             "logs/access.log")
    ctx.client_max_body_size    = get(config, "nginx.client_max_body_size",   0)

    -- OpenResty mime.types (绝对路径)
    local mime_path = detect_mime_types_path()
    if not mime_path then
        print("Generate nginx.conf    ...FAIL (cannot find OpenResty mime.types)")
        os.exit(1)
    end
    ctx.mime_types_path = mime_path

    -- 占位 SSL 证书 (自动生成)
    local cert, key = ensure_placeholder_ssl(home)
    ctx.ssl_certificate     = cert
    ctx.ssl_certificate_key = key

    -- 复合块
    ctx.resolver     = build_resolver(config)
    ctx.shared_dicts = build_shared_dicts(get(config, "nginx.shared_dict", {}))
    ctx.admin_server = build_admin_server(config)

    -- Listen 指令
    local http_ports  = get(config, "nginx.http_listen",  { 10080 })
    local https_ports = get(config, "nginx.https_listen", { 10443 })
    ctx.http_listen   = build_listen(http_ports)
    ctx.https_listen  = build_listen(https_ports, "ssl")

    return ctx
end

-- ============================================================
-- 公共 API
-- ============================================================

--- 主入口: 读取 nyro.yaml + 模板 → 渲染 → 写入 conf/nginx.conf
function _M.generate()
    local home = common.nyro_home

    -- 1. 读取 nyro.yaml
    local yaml_path = home .. "/conf/nyro.yaml"
    local f, err = io.open(yaml_path, "r")
    if not f then
        print("Generate nginx.conf    ...FAIL (cannot read " .. yaml_path .. ": " .. (err or "") .. ")")
        os.exit(1)
    end
    local yaml_content = f:read("*a")
    f:close()

    local yaml = require("tinyyaml")
    local config = yaml.parse(yaml_content)
    if not config then
        print("Generate nginx.conf    ...FAIL (invalid nyro.yaml)")
        os.exit(1)
    end

    -- 2. 加载模板 (从 Lua 模块)
    local template = require("nyro.cli.templates.nginx_conf")

    -- 3. 渲染
    local context = build_context(config, home)
    local ok, result = pcall(render, template, context)
    if not ok then
        print("Generate nginx.conf    ...FAIL (" .. tostring(result) .. ")")
        os.exit(1)
    end

    -- 4. 写入 conf/nginx.conf
    local out_path = home .. "/conf/nginx.conf"
    local of, oerr = io.open(out_path, "w")
    if not of then
        print("Generate nginx.conf    ...FAIL (cannot write " .. out_path .. ": " .. (oerr or "") .. ")")
        os.exit(1)
    end
    of:write(result)
    of:close()

    print("Generate nginx.conf    ...OK")
end

return _M
