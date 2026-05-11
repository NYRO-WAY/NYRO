# Race Gateway Load Test And Metrics

本文档用于本地开发和生产压测时快速验证 `race-gateway` 的竞速路径、流式首包时延和管理面指标。

## Metrics

管理端暴露 Prometheus 文本指标：

- `GET /admin/metrics`
- Content-Type: `text/plain; version=0.0.4; charset=utf-8`

当前重点指标：

- `race_gateway_http_requests_total`
  - 按 `surface=proxy|admin`、`route`、`method`、`status` 统计请求量
- `race_gateway_http_request_duration_seconds`
  - 按 `surface`、`route`、`method` 统计 HTTP 耗时
- `race_gateway_active_races`
  - 按 `protocol` 统计当前活跃竞速请求数
- `race_gateway_races_total`
  - 按 `protocol`、`outcome=winner|all_failed` 统计完成的竞速数
- `race_gateway_race_duration_seconds`
  - 按 `protocol`、`outcome` 统计端到端竞速时长

开发期快速查看：

```bash
curl http://127.0.0.1:3401/admin/metrics
```

## Built-in Load Test

仓库内置了一个轻量压测二进制：

```bash
cargo run --bin race-loadtest -- \
  --url http://127.0.0.1:3400/groups/demo/openai/v1/chat/completions \
  --requests 200 \
  --concurrency 20 \
  --timeout-ms 60000 \
  --header content-type:application/json \
  --header x-api-key:test-key \
  --body-file examples/loadtest/openai-chat.json
```

输出会包含：

- `headers_ms`
  - 收到响应头的延迟
- `first_chunk_ms`
  - 收到首个流式 chunk 的延迟
- `total_ms`
  - 读完整个响应流的总耗时
- `status`
  - HTTP 状态分布
- `throughput`
  - 请求吞吐

适用场景：

- 比较不同 `group` 的竞速首包时延
- 验证 `response_protection_timeout_ms` 调整后的首包影响
- 观察流式长响应是否被错误截断
- 配合 `/admin/metrics` 对照活跃竞速和耗时分布

## Minimal Body Example

建议先准备一个最小请求体文件，例如 `examples/loadtest/openai-chat.json`：

```json
{
  "model": "placeholder",
  "stream": true,
  "messages": [
    {
      "role": "user",
      "content": "Say hello in one short sentence."
    }
  ]
}
```

说明：

- `model` 字段会在 `race-gateway` 下游发起请求前按目标 candidate 的 `upstream_model` 覆盖。
- OpenAI / Anthropic 路由建议显式带 `stream: true`。
- Google 路由请准备对应协议格式的 body 文件。
