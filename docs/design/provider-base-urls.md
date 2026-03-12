# 模型供应商对接地址清单

> 用途：Nyro 后续对接供应商时的统一地址参考。  
> 约定：Global 和 China 合并在同一单元格展示；不支持的协议填 `-`。

| 供应商 | 平台地址 | OpenAI Base URL | Anthropic Base URL | Gemini Base URL | 鉴权/调用备注 |
|---|---|---|---|---|---|
| Google | Global: [ai.google.dev](https://ai.google.dev)<br>China: - | Global: `https://generativelanguage.googleapis.com/v1beta/openai`<br>China: - | - | Global: `https://generativelanguage.googleapis.com`<br>China: - | OpenAI: `Authorization: Bearer <API_KEY>`；Gemini: URL query `?key=<API_KEY>` |
| xAI | Global: [x.ai](https://x.ai)<br>China: - | Global: `https://api.x.ai/v1`<br>China: - | - | - | OpenAI: `Authorization: Bearer <API_KEY>` |
| DeepSeek | Global: [platform.deepseek.com](https://platform.deepseek.com)<br>China: - | Global: `https://api.deepseek.com/v1`<br>China: - | Global: `https://api.deepseek.com/anthropic`<br>China: - | - | OpenAI: `Authorization: Bearer <API_KEY>`；Anthropic: `x-api-key` + `anthropic-version: 2023-06-01` |
| Kimi | Global: [platform.moonshot.ai](https://platform.moonshot.ai)<br>China: [platform.moonshot.cn](https://platform.moonshot.cn) | Global: `https://api.moonshot.ai/v1`<br>China: `https://api.moonshot.cn/v1` | Global: `https://api.moonshot.ai/anthropic`<br>China: `https://api.moonshot.cn/anthropic` | - | OpenAI: `Authorization: Bearer <API_KEY>`；Anthropic: `x-api-key` + `anthropic-version: 2023-06-01` |
| MiniMax | Global: [platform.minimax.io](https://platform.minimax.io)<br>China: [platform.minimaxi.com](https://platform.minimaxi.com) | Global: `https://api.minimax.io/v1`<br>China: `https://api.minimaxi.com/v1` | Global: `https://api.minimax.io/anthropic`<br>China: `https://api.minimaxi.com/anthropic` | - | OpenAI: `Authorization: Bearer <API_KEY>`；Anthropic: `x-api-key` + `anthropic-version: 2023-06-01` |
| Zhipu | Global: [z.ai](https://z.ai)<br>China: [bigmodel.cn](https://bigmodel.cn) | Global: `https://api.z.ai/api/paas/v4`<br>China: `https://open.bigmodel.cn/api/paas/v4` | Global: `https://api.z.ai/api/anthropic`<br>China: `https://open.bigmodel.cn/api/anthropic` | - | OpenAI: `Authorization: Bearer <API_KEY>`；Anthropic: `x-api-key` + `anthropic-version: 2023-06-01` |
| NVIDIA | Global: [build.nvidia.com](https://build.nvidia.com)<br>China: - | Global: `https://integrate.api.nvidia.com/v1`<br>China: - | - | - | OpenAI: `Authorization: Bearer <API_KEY>` |
| OpenRouter | Global: [openrouter.ai](https://openrouter.ai)<br>China: - | Global: `https://openrouter.ai/api/v1`<br>China: - | Global: `https://openrouter.ai/api`<br>China: - | - | OpenAI: `Authorization: Bearer <API_KEY>`；Anthropic: `x-api-key` + `anthropic-version: 2023-06-01` |
| Ollama | Global: [ollama.com](https://ollama.com)<br>China: - | Global: `http://127.0.0.1:11434/v1`<br>China: - | - | - | 默认无鉴权；若启用鉴权按 OpenAI Bearer 方式处理 |
