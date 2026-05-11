# Gemini-first example

This directory contains a minimal end-to-end cutover example for:

```text
Claude Code / Anthropic client -> nyro -> upstream-gateway -> Gemini
```

Files:

- `bootstrap.template.json`
  - persisted provider/key/model-rule seed for `upstream-gateway`
- `nyro.standalone.yaml`
  - minimal standalone Nyro config that points Gemini egress at `upstream-gateway`
- `validate.ps1`
  - Windows PowerShell validation script that starts both services and checks a real Gemini request

Important note:

- the `api_key` in `nyro.standalone.yaml` is only the Nyro -> upstream-gateway hop credential placeholder
- the real Gemini API key lives in `upstream-gateway` key pool config
