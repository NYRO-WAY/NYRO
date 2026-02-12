package = "nyro"
version = "master-0"
rockspec_format = "3.0"
supported_platforms = {"linux", "macosx"}

source = {
    url = "git://github.com/nyro/nyro",
    branch = "master",
}

description = {
    summary = "NYRO provides full life cycle management of API release, management, and operation and maintenance.",
    homepage = "https://github.com/nyro/nyro",
    license = "Apache License 2.0",
    maintainer = "Janko <shuaijinchao@gmail.com>"
}

dependencies = {
    "lua-resty-worker-events == 2.0.1-1",
    "lua-resty-balancer == 0.02rc5",
    "lua-resty-jwt == 0.2.0",
    "lua-resty-http == 0.15-0",
    "lua-resty-lrucache == 0.09-2",
    "jsonschema == 0.9.8-0",
    "luasocket == 3.0rc1-2",
    "luafilesystem == 1.7.0-2",
    "lua-tinyyaml == 0.1",
    "multipart == 0.5.5-1",
    "penlight == 1.5.4-1",
    "lua-resty-jit-uuid == 0.0.7-2",
    "lua-resty-dns == 0.21-1"
}

build = {
    type = "builtin",
    modules = {
        -- Main entry
        ["nyro"] = "nyro/nyro.lua",

        -- Core modules
        ["nyro.core"] = "nyro/core/init.lua",
        ["nyro.core.const"] = "nyro/core/const.lua",
        ["nyro.core.ctx"] = "nyro/core/ctx.lua",
        ["nyro.core.json"] = "nyro/core/json.lua",
        ["nyro.core.log"] = "nyro/core/log.lua",
        ["nyro.core.request"] = "nyro/core/request.lua",
        ["nyro.core.response"] = "nyro/core/response.lua",
        ["nyro.core.schema"] = "nyro/core/schema.lua",
        ["nyro.core.shared"] = "nyro/core/shared.lua",
        ["nyro.core.config"] = "nyro/core/config.lua",
        ["nyro.core.cache"] = "nyro/core/cache.lua",
        ["nyro.core.meta"] = "nyro/core/meta.lua",
        ["nyro.core.utils.string"] = "nyro/core/utils/string.lua",
        ["nyro.core.utils.table"] = "nyro/core/utils/table.lua",
        ["nyro.core.utils.tablepool"] = "nyro/core/utils/tablepool.lua",
        ["nyro.core.utils.time"] = "nyro/core/utils/time.lua",

        -- FFI modules
        ["nyro.ffi"] = "nyro/ffi/init.lua",
        ["nyro.ffi.llm"] = "nyro/ffi/llm.lua",

        -- Route modules (routes resource)
        ["nyro.route"] = "nyro/route/init.lua",
        ["nyro.route.matcher"] = "nyro/route/matcher.lua",
        ["nyro.route.ffi"] = "nyro/route/ffi.lua",

        -- Backend modules (backends resource)
        ["nyro.backend"] = "nyro/backend/init.lua",

        -- Service modules (services resource)
        ["nyro.service"] = "nyro/service/init.lua",

        -- Consumer modules (consumers resource)
        ["nyro.consumer"] = "nyro/consumer/init.lua",

        -- Certificate modules (certificates resource)
        ["nyro.certificate"] = "nyro/certificate/init.lua",

        -- Plugin modules (plugins resource)
        ["nyro.plugin"] = "nyro/plugin/init.lua",
        ["nyro.plugin.common"] = "nyro/plugin/common.lua",
        ["nyro.plugin.cors.handler"] = "nyro/plugin/cors/handler.lua",
        ["nyro.plugin.cors.schema"] = "nyro/plugin/cors/schema.lua",
        ["nyro.plugin.jwt-auth.handler"] = "nyro/plugin/jwt-auth/handler.lua",
        ["nyro.plugin.jwt-auth.schema"] = "nyro/plugin/jwt-auth/schema.lua",
        ["nyro.plugin.key-auth.handler"] = "nyro/plugin/key-auth/handler.lua",
        ["nyro.plugin.key-auth.schema"] = "nyro/plugin/key-auth/schema.lua",
        ["nyro.plugin.limit-conn.handler"] = "nyro/plugin/limit-conn/handler.lua",
        ["nyro.plugin.limit-conn.schema"] = "nyro/plugin/limit-conn/schema.lua",
        ["nyro.plugin.limit-count.handler"] = "nyro/plugin/limit-count/handler.lua",
        ["nyro.plugin.limit-count.schema"] = "nyro/plugin/limit-count/schema.lua",
        ["nyro.plugin.limit-req.handler"] = "nyro/plugin/limit-req/handler.lua",
        ["nyro.plugin.limit-req.schema"] = "nyro/plugin/limit-req/schema.lua",
        ["nyro.plugin.mock.handler"] = "nyro/plugin/mock/handler.lua",
        ["nyro.plugin.mock.schema"] = "nyro/plugin/mock/schema.lua",
        ["nyro.plugin.ai-proxy.handler"] = "nyro/plugin/ai-proxy/handler.lua",
        ["nyro.plugin.ai-proxy.schema"] = "nyro/plugin/ai-proxy/schema.lua",

        -- Admin API modules
        ["nyro.admin"] = "nyro/admin/init.lua",
        ["nyro.admin.helper"] = "nyro/admin/helper.lua",
        ["nyro.admin.routes"] = "nyro/admin/routes.lua",
        ["nyro.admin.services"] = "nyro/admin/services.lua",
        ["nyro.admin.backends"] = "nyro/admin/backends.lua",
        ["nyro.admin.consumers"] = "nyro/admin/consumers.lua",
        ["nyro.admin.plugins"] = "nyro/admin/plugins.lua",
        ["nyro.admin.certificates"] = "nyro/admin/certificates.lua",
        ["nyro.admin.config"] = "nyro/admin/config.lua",
        ["nyro.admin.status"] = "nyro/admin/status.lua",

        -- Schema modules
        ["nyro.schema"] = "nyro/schema/init.lua",
        ["nyro.schema.common"] = "nyro/schema/common.lua",
        ["nyro.schema.backend"] = "nyro/schema/backend.lua",
        ["nyro.schema.service"] = "nyro/schema/service.lua",
        ["nyro.schema.route"] = "nyro/schema/route.lua",
        ["nyro.schema.plugin"] = "nyro/schema/plugin.lua",
        ["nyro.schema.certificate"] = "nyro/schema/certificate.lua",
        ["nyro.schema.upstream_node"] = "nyro/schema/upstream_node.lua",

        -- Store modules
        ["nyro.store"] = "nyro/store/init.lua",
        ["nyro.store.adapter.yaml"] = "nyro/store/adapter/yaml.lua",

        -- CLI modules
        ["nyro.cli"] = "nyro/cli/init.lua",
        ["nyro.cli.env"] = "nyro/cli/env.lua",
        ["nyro.cli.help"] = "nyro/cli/help.lua",
        ["nyro.cli.init"] = "nyro/cli/init.lua",
        ["nyro.cli.quit"] = "nyro/cli/quit.lua",
        ["nyro.cli.reload"] = "nyro/cli/reload.lua",
        ["nyro.cli.restart"] = "nyro/cli/restart.lua",
        ["nyro.cli.start"] = "nyro/cli/start.lua",
        ["nyro.cli.stop"] = "nyro/cli/stop.lua",
        ["nyro.cli.test"] = "nyro/cli/test.lua",
        ["nyro.cli.version"] = "nyro/cli/version.lua",
        ["nyro.cli.utils.common"] = "nyro/cli/utils/common.lua",
        ["nyro.cli.utils.kill"] = "nyro/cli/utils/kill.lua",
        ["nyro.cli.utils.nginx_signals"] = "nyro/cli/utils/nginx_signals.lua",
        ["nyro.cli.generator"] = "nyro/cli/generator.lua",
        ["nyro.cli.templates.nginx_conf"] = "nyro/cli/templates/nginx_conf.lua",
    },
}
