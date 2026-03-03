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

    -- Console 静态目录 (默认 console，可配置为 webui/dist)
    local console_ui = get(config, "admin.console.ui", "console")
    console_ui = tostring(console_ui):gsub("/+$", "")

    -- local-* API 反向代理目标端口 (动态取 nginx.http_listen 第一项)
    local proxy_http_listen = get(config, "nginx.http_listen", { 10080 })
    if type(proxy_http_listen) ~= "table" then
        proxy_http_listen = { proxy_http_listen }
    end
    local proxy_port = tostring(proxy_http_listen[1] or 10080)

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
        "        location /nyro/local/ {",
        "            proxy_http_version 1.1;",
        "            proxy_set_header   Host              $host;",
        "            proxy_set_header   X-Real-IP         $remote_addr;",
        "            proxy_set_header   X-Request-Id      $request_id;",
        "            proxy_pass http://127.0.0.1:" .. proxy_port .. ";",
        "        }",
        "",
        "        location / {",
        "            index index.html;",
        "            alias " .. console_ui .. "/;",
        "",
        "            try_files $uri $uri/ /index.html;",
        "        }",
        "    }",
    }, "\n")
end

--- 检查 nyro.yaml plugins 列表中是否包含指定插件
local function has_plugin(config, plugin_name)
    local plugins = config.plugins
    if not plugins or type(plugins) ~= "table" then
        return false
    end
    for _, name in ipairs(plugins) do
        if name == plugin_name then
            return true
        end
    end
    return false
end

--- 构建可观测插件相关的 nginx 指令块
local function build_observability(config)
    local has_logs    = has_plugin(config, "local-logs")
    local has_metrics = has_plugin(config, "local-metrics")

    -- http 块: log_format nyro_json (仅 local-logs 启用时)
    local log_format_block = ""
    if has_logs then
        log_format_block = table.concat({
            "",
            "    log_format nyro_json escape=json '{'",
            "    '\"timestamp\":\"$time_iso8601\",'",
            "    '\"client_ip\":\"$remote_addr\",'",
            "    '\"method\":\"$request_method\",'",
            "    '\"uri\":\"$uri\",'",
            "    '\"status\":$status,'",
            "    '\"latency_ms\":$request_time,'",
            "    '\"request_length\":$request_length,'",
            "    '\"response_length\":$bytes_sent,'",
            "    '\"upstream_addr\":\"$upstream_addr\",'",
            "    '\"upstream_status\":\"$upstream_status\",'",
            "    '\"request_id\":\"$request_id\",'",
            "    '\"route\":\"$nyro_route\",'",
            "    '\"service\":\"$nyro_service\",'",
            "    '\"consumer\":\"$nyro_consumer\",'",
            "    '\"model\":\"$nyro_model\",'",
            "    '\"input_tokens\":$nyro_input_tokens,'",
            "    '\"output_tokens\":$nyro_output_tokens'",
            "    '}';",
        }, "\n")
    end

    -- location / 内: set 变量 + access_log (仅 local-logs 启用时)
    local location_vars_block = ""
    if has_logs then
        location_vars_block = table.concat({
            "",
            "            # local-logs: 自定义变量 (由 handler.lua 在 log 阶段赋值)",
            "            set $nyro_route              '';",
            "            set $nyro_service            '';",
            "            set $nyro_consumer           '';",
            "            set $nyro_model              '';",
            "            set $nyro_input_tokens   '0';",
            "            set $nyro_output_tokens  '0';",
            "",
            "            # local-logs: JSON 日志输出",
            "            access_log logs/access.json nyro_json;",
        }, "\n")
    end

    -- 可观测 location 块
    local locations = {}
    if has_metrics then
        locations[#locations + 1] = table.concat({
            "",
            "        location /nyro/local/metrics {",
            "            content_by_lua_block {",
            "                nyro.http_local_metrics()",
            "            }",
            "        }",
        }, "\n")
    end
    if has_logs then
        locations[#locations + 1] = table.concat({
            "",
            "        location /nyro/local/logs {",
            "            content_by_lua_block {",
            "                nyro.http_local_logs()",
            "            }",
            "        }",
        }, "\n")
    end

    return {
        log_format_block     = log_format_block,
        location_vars_block  = location_vars_block,
        location_blocks      = table.concat(locations, "\n"),
    }
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

    -- 可观测插件 (条件化生成)
    local obs = build_observability(config)
    ctx.observability_log_format  = obs.log_format_block
    ctx.observability_vars        = obs.location_vars_block
    ctx.observability_locations   = obs.location_blocks

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
