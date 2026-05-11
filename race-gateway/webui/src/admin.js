const $ = (id) => document.getElementById(id);

const state = {
  health: null,
  models: [],
  groups: [],
  keyPools: [],
  runtime: [],
  settings: null,
  selected: {
    models: "",
    groups: "",
    "key-pools": "",
  },
};

const authKinds = [
  { value: "bearer", label: "Bearer", extraLabel: "" },
  { value: "header_api_key", label: "Header API Key", extraLabel: "请求头名称" },
  { value: "query_api_key", label: "Query API Key", extraLabel: "Query 参数名" },
];

const protocolKinds = [
  { value: "openai", label: "OpenAI" },
  { value: "anthropic", label: "Anthropic" },
  { value: "google", label: "Google" },
];

const resourceConfigs = {
  models: {
    path: "models",
    validatePath: "model",
    summaryLabel: "模型",
    listId: "modelsList",
    searchId: "modelSearch",
    template: (id) => ({
      id,
      display_name: "新模型",
      upstream_model: "",
      description: "",
      enabled: true,
      endpoints: [defaultEndpoint()],
      metadata: {},
    }),
  },
  groups: {
    path: "groups",
    validatePath: "group",
    summaryLabel: "竞速组",
    listId: "groupsList",
    searchId: "groupSearch",
    template: (id) => ({
      id,
      display_name: "新竞速组",
      fallback_ratio: 0.5,
      decay_factor: 0.8,
      penalty_rate: 5,
      recovery_rate: 0.2,
      race_max_wait_time_ms: 15000,
      enabled: true,
      candidates: [defaultCandidate(id)],
    }),
  },
  "key-pools": {
    path: "key-pools",
    validatePath: "key-pool",
    summaryLabel: "Key 池",
    listId: "keyPoolsList",
    searchId: "keyPoolSearch",
    template: (id) => ({
      id,
      display_name: "新 Key 池",
      auth_strategy: { kind: "bearer" },
      selection_strategy: "random",
      enabled: true,
      keys: [defaultKey(id)],
    }),
  },
};

function defaultEndpoint() {
  const defaultKeyPoolId = state.keyPools[0]?.id || "";
  return {
    protocol_family: "openai",
    base_url: "",
    auth_strategy: { kind: "bearer" },
    key_pool_id: defaultKeyPoolId,
    request_timeout_ms: 30000,
    extra_headers: {},
    extra_query: {},
    enabled: true,
  };
}

function defaultCandidate(groupId = "group-example") {
  return {
    id: `${groupId}-candidate-${Date.now()}`,
    group_id: groupId,
    name: "新候选",
    model_id: "",
    upstream_model: "",
    inline_endpoint_overrides: [],
    initial_weight: 100,
    response_protection_timeout_ms: 5000,
    enabled: true,
    metadata: {},
  };
}

function defaultKey(poolId = "keypool-example") {
  return {
    id: `${poolId}-key-${Date.now()}`,
    key_pool_id: poolId,
    secret: "",
    enabled: true,
    metadata: {},
  };
}

async function fetchJson(url, options) {
  const res = await fetch(url, options);
  const text = await res.text();
  let data = null;
  if (text) {
    try {
      data = JSON.parse(text);
    } catch {
      data = null;
    }
  }
  if (!res.ok) {
    const issueSummary = Array.isArray(data?.issues) && data.issues.length
      ? data.issues
        .slice(0, 3)
        .map((issue) => `${issue.field || "字段"}：${issue.message || issue.code || "校验失败"}`)
        .join("；")
      : "";
    const error = new Error(data?.message || issueSummary || data?.error?.message || text || `${res.status}`);
    error.status = res.status;
    error.payload = data;
    throw error;
  }
  return data;
}

function setFlash(message, level = "info") {
  const el = $("flash");
  el.textContent = message;
  el.className = `flash ${level}`;
  clearTimeout(setFlash.timer);
  setFlash.timer = setTimeout(() => {
    el.className = "flash hidden";
    el.textContent = "";
  }, 4000);
}

function switchTab(name) {
  document.querySelectorAll("[data-tab]").forEach((button) => {
    button.classList.toggle("active", button.dataset.tab === name);
  });
  document.querySelectorAll("[data-panel]").forEach((panel) => {
    panel.classList.toggle("active", panel.dataset.panel === name);
  });
}

function pretty(value) {
  return JSON.stringify(value ?? {}, null, 2);
}

function escapeHtml(value) {
  return String(value ?? "")
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;");
}

function normalizedSearchValue(id) {
  return ($(id)?.value || "").trim().toLowerCase();
}

function matchesSearch(value, query) {
  return !query || String(value || "").toLowerCase().includes(query);
}

function formatProtocol(value) {
  return protocolKinds.find((item) => item.value === value)?.label || value || "未知";
}

function formatStatus(value) {
  const labels = {
    healthy: "正常",
    recovering: "恢复中",
    penalized: "惩罚中",
  };
  return labels[value] || value || "未知";
}

function statusBadgeClass(enabled, status) {
  if (enabled === false || status === "penalized") return "danger";
  if (status === "recovering") return "warn";
  return "ok";
}

function badge(text, kind = "") {
  return `<span class="badge ${kind}">${escapeHtml(text)}</span>`;
}

function clearIssues(name) {
  const issuesId = {
    models: "modelIssues",
    groups: "groupIssues",
    "key-pools": "keyPoolIssues",
  }[name];
  $(issuesId).innerHTML = "";
}

function renderIssues(name, payload) {
  const issuesId = {
    models: "modelIssues",
    groups: "groupIssues",
    "key-pools": "keyPoolIssues",
  }[name];
  const root = $(issuesId);
  root.innerHTML = "";
  const issues = payload?.issues || [];
  if (!issues.length) {
    root.innerHTML = `<div class="issue"><strong>校验通过</strong><span>当前配置已通过字段校验。</span></div>`;
    return;
  }
  issues.forEach((issue) => {
    const item = document.createElement("div");
    item.className = "issue";
    item.innerHTML = `<strong>${escapeHtml(issue.field || "未知字段")} · ${escapeHtml(issue.code || "invalid")}</strong><span>${escapeHtml(issue.message || "校验失败")}</span>`;
    root.appendChild(item);
  });
}

function parseJsonOrThrow(text, label, fallback = {}) {
  const raw = (text || "").trim();
  if (!raw) return fallback;
  try {
    return JSON.parse(raw);
  } catch (error) {
    throw new Error(`${label} 不是合法 JSON：${error.message}`);
  }
}

function numberValue(id, fallback = 0) {
  const raw = $(id).value.trim();
  return raw === "" ? fallback : Number(raw);
}

function authStrategyToFields(strategy) {
  const kind = strategy?.kind || "bearer";
  if (kind === "header_api_key") {
    return { kind, extra: strategy.header_name || "" };
  }
  if (kind === "query_api_key") {
    return { kind, extra: strategy.parameter_name || "" };
  }
  return { kind: "bearer", extra: "" };
}

function authStrategyFromFields(kind, extra) {
  if (kind === "header_api_key") {
    return { kind, header_name: extra || "x-api-key" };
  }
  if (kind === "query_api_key") {
    return { kind, parameter_name: extra || "key" };
  }
  return { kind: "bearer" };
}

function renderAuthKindOptions(selected) {
  return authKinds
    .map((item) => `<option value="${item.value}" ${item.value === selected ? "selected" : ""}>${item.label}</option>`)
    .join("");
}

function renderProtocolOptions(selected) {
  return protocolKinds
    .map((item) => `<option value="${item.value}" ${item.value === selected ? "selected" : ""}>${item.label}</option>`)
    .join("");
}

function renderKeyPoolOptions(selected) {
  const selectedValue = selected || state.keyPools[0]?.id || "";
  if (!state.keyPools.length) {
    return `<option value="">请先创建 Key 池</option>`;
  }

  const options = state.keyPools.map((pool) => {
    const suffix = pool.enabled === false ? "（已禁用）" : "";
    return `<option value="${escapeHtml(pool.id)}" ${pool.id === selectedValue ? "selected" : ""}>${escapeHtml(pool.display_name || pool.id)} · ${escapeHtml(pool.id)}${suffix}</option>`;
  });

  if (selectedValue && !state.keyPools.some((pool) => pool.id === selectedValue)) {
    options.unshift(`<option value="${escapeHtml(selectedValue)}" selected>${escapeHtml(selectedValue)}（不存在）</option>`);
  }

  return options.join("");
}

function renderModelOptions(selected, { allowEmpty = true } = {}) {
  const options = [];
  if (allowEmpty) {
    options.push(`<option value="" ${!selected ? "selected" : ""}>不绑定模型，直接使用候选自带上游模型</option>`);
  }

  state.models.forEach((model) => {
    const suffix = model.enabled === false ? "（已禁用）" : "";
    options.push(
      `<option value="${escapeHtml(model.id)}" ${model.id === selected ? "selected" : ""}>${escapeHtml(model.display_name || model.id)} · ${escapeHtml(model.id)}${suffix}</option>`
    );
  });

  if (selected && !state.models.some((model) => model.id === selected)) {
    options.unshift(`<option value="${escapeHtml(selected)}" selected>${escapeHtml(selected)}（不存在）</option>`);
  }

  return options.join("");
}

function modelById(modelId) {
  return state.models.find((model) => model.id === modelId) || null;
}

function candidateResolvedUpstreamModel(candidate) {
  const explicit = (candidate?.upstream_model || "").trim();
  if (explicit) return explicit;
  const boundModel = modelById((candidate?.model_id || "").trim());
  return boundModel?.upstream_model || "";
}

function generateCandidateId(groupId, candidateName, index) {
  const slug = String(candidateName || "")
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");

  return slug ? `${groupId}-${slug}` : `${groupId}-candidate-${index + 1}`;
}

function valueTypeOf(value) {
  if (value === null) return "null";
  if (Array.isArray(value) || typeof value === "object") return "json";
  if (typeof value === "number") return "number";
  if (typeof value === "boolean") return "boolean";
  return "string";
}

function valueTextOf(value, type = valueTypeOf(value)) {
  if (type === "json") return pretty(value);
  if (type === "null") return "";
  return String(value ?? "");
}

function normalizeMetadataEntries(value) {
  return Object.entries(value || {}).map(([key, entryValue]) => ({
    key,
    type: valueTypeOf(entryValue),
    value: valueTextOf(entryValue),
  }));
}

function renderMetadataRowsMarkup(value) {
  const entries = normalizeMetadataEntries(value);
  if (!entries.length) {
    return `<div class="resource-empty">当前没有 metadata 字段，点击“新增字段”添加。</div>`;
  }

  return entries
    .map((entry) => `
      <div class="kv-row metadata-row">
        <input data-meta-field="key" placeholder="字段名" value="${escapeHtml(entry.key)}" />
        <select data-meta-field="type">
          <option value="string" ${entry.type === "string" ? "selected" : ""}>字符串</option>
          <option value="number" ${entry.type === "number" ? "selected" : ""}>数字</option>
          <option value="boolean" ${entry.type === "boolean" ? "selected" : ""}>布尔</option>
          <option value="null" ${entry.type === "null" ? "selected" : ""}>空值</option>
          <option value="json" ${entry.type === "json" ? "selected" : ""}>JSON</option>
        </select>
        <textarea data-meta-field="value" class="kv-textarea" placeholder="字段值">${escapeHtml(entry.value)}</textarea>
        <button type="button" data-action="remove-metadata-row" class="danger">删除</button>
      </div>
    `)
    .join("");
}

function appendMetadataRow(root, entry = { key: "", type: "string", value: "" }) {
  const empty = root.querySelector(".resource-empty");
  if (empty) empty.remove();
  root.insertAdjacentHTML(
    "beforeend",
    `
      <div class="kv-row metadata-row">
        <input data-meta-field="key" placeholder="字段名" value="${escapeHtml(entry.key || "")}" />
        <select data-meta-field="type">
          <option value="string" ${entry.type === "string" ? "selected" : ""}>字符串</option>
          <option value="number" ${entry.type === "number" ? "selected" : ""}>数字</option>
          <option value="boolean" ${entry.type === "boolean" ? "selected" : ""}>布尔</option>
          <option value="null" ${entry.type === "null" ? "selected" : ""}>空值</option>
          <option value="json" ${entry.type === "json" ? "selected" : ""}>JSON</option>
        </select>
        <textarea data-meta-field="value" class="kv-textarea" placeholder="字段值">${escapeHtml(entry.value || "")}</textarea>
        <button type="button" data-action="remove-metadata-row" class="danger">删除</button>
      </div>
    `
  );
}

function collectMetadataRows(root, label) {
  const result = {};
  root.querySelectorAll(".metadata-row").forEach((row, index) => {
    const key = row.querySelector('[data-meta-field="key"]').value.trim();
    const type = row.querySelector('[data-meta-field="type"]').value;
    const rawValue = row.querySelector('[data-meta-field="value"]').value;
    if (!key) return;

    try {
      if (type === "number") {
        result[key] = Number(rawValue);
      } else if (type === "boolean") {
        if (rawValue.trim() === "") {
          result[key] = false;
        } else if (/^(true|false)$/i.test(rawValue.trim())) {
          result[key] = rawValue.trim().toLowerCase() === "true";
        } else {
          throw new Error("布尔值只能填写 true 或 false");
        }
      } else if (type === "null") {
        result[key] = null;
      } else if (type === "json") {
        result[key] = parseJsonOrThrow(rawValue, `${label} 第 ${index + 1} 项`, {});
      } else {
        result[key] = rawValue;
      }
    } catch (error) {
      throw new Error(`${label} 字段 ${key} 解析失败：${error.message}`);
    }
  });
  return result;
}

function renderStringMapRowsMarkup(value, keyPlaceholder = "键", valuePlaceholder = "值") {
  const entries = Object.entries(value || {});
  if (!entries.length) {
    return `<div class="resource-empty">当前没有配置项，点击“新增”添加。</div>`;
  }

  return entries
    .map(([key, mapValue]) => `
      <div class="kv-row map-row">
        <input data-map-field="key" placeholder="${escapeHtml(keyPlaceholder)}" value="${escapeHtml(key)}" />
        <input data-map-field="value" placeholder="${escapeHtml(valuePlaceholder)}" value="${escapeHtml(mapValue)}" />
        <button type="button" data-action="remove-map-row" class="danger">删除</button>
      </div>
    `)
    .join("");
}

function appendStringMapRow(root, keyPlaceholder = "键", valuePlaceholder = "值", entry = { key: "", value: "" }) {
  const empty = root.querySelector(".resource-empty");
  if (empty) empty.remove();
  root.insertAdjacentHTML(
    "beforeend",
    `
      <div class="kv-row map-row">
        <input data-map-field="key" placeholder="${escapeHtml(keyPlaceholder)}" value="${escapeHtml(entry.key || "")}" />
        <input data-map-field="value" placeholder="${escapeHtml(valuePlaceholder)}" value="${escapeHtml(entry.value || "")}" />
        <button type="button" data-action="remove-map-row" class="danger">删除</button>
      </div>
    `
  );
}

function collectStringMapRows(root) {
  const result = {};
  root.querySelectorAll(".map-row").forEach((row) => {
    const key = row.querySelector('[data-map-field="key"]').value.trim();
    const value = row.querySelector('[data-map-field="value"]').value;
    if (!key) return;
    result[key] = value;
  });
  return result;
}

function renderEndpointEditorMarkup(endpoint, index, removeAction, heading = "端点") {
  const auth = authStrategyToFields(endpoint.auth_strategy);
  const extraLabel = authKinds.find((item) => item.value === auth.kind)?.extraLabel || "附加参数";
  return `
    <article class="subitem-card endpoint-card" data-endpoint-index="${index}">
      <div class="subitem-head">
        <div>
          <h5>${heading} ${index + 1}</h5>
          <p class="resource-item-subtitle">${escapeHtml(formatProtocol(endpoint.protocol_family))}</p>
        </div>
        <button type="button" data-action="${removeAction}" data-index="${index}" class="danger">删除端点</button>
      </div>
      <div class="subitem-grid three-cols">
        <label class="field">
          <span>协议</span>
          <select data-field="protocol_family">${renderProtocolOptions(endpoint.protocol_family)}</select>
        </label>
        <label class="field">
          <span>Key 池</span>
          <select data-field="key_pool_id">${renderKeyPoolOptions(endpoint.key_pool_id || "")}</select>
        </label>
        <label class="field">
          <span>请求超时（毫秒）</span>
          <input data-field="request_timeout_ms" type="number" min="0" value="${escapeHtml(endpoint.request_timeout_ms ?? "")}" />
        </label>
      </div>
      <label class="field">
        <span>上游 Base URL</span>
        <input data-field="base_url" value="${escapeHtml(endpoint.base_url || "")}" />
      </label>
      <div class="subitem-grid three-cols">
        <label class="field">
          <span>认证策略</span>
          <select data-field="auth_kind">${renderAuthKindOptions(auth.kind)}</select>
        </label>
        <label class="field">
          <span class="auth-extra-label">${escapeHtml(extraLabel || "附加参数")}</span>
          <input data-field="auth_extra" value="${escapeHtml(auth.extra || "")}" />
        </label>
        <label class="field checkbox-field">
          <span>启用状态</span>
          <label class="checkbox-inline">
            <input data-field="enabled" type="checkbox" ${endpoint.enabled !== false ? "checked" : ""} />
            <span>已启用</span>
          </label>
        </label>
      </div>
      <div class="subitem-grid two-cols">
        <div class="field">
          <div class="inline-head">
            <span>附加请求头</span>
            <button type="button" data-action="add-map-row" data-target="extra_headers">新增请求头</button>
          </div>
          <div class="kv-editor" data-map-group="extra_headers">
            ${renderStringMapRowsMarkup(endpoint.extra_headers || {}, "Header 名称", "Header 值")}
          </div>
        </div>
        <div class="field">
          <div class="inline-head">
            <span>附加 Query 参数</span>
            <button type="button" data-action="add-map-row" data-target="extra_query">新增参数</button>
          </div>
          <div class="kv-editor" data-map-group="extra_query">
            ${renderStringMapRowsMarkup(endpoint.extra_query || {}, "参数名", "参数值")}
          </div>
        </div>
      </div>
    </article>
  `;
}

function collectEndpointCard(card, label) {
  const kind = card.querySelector('[data-field="auth_kind"]').value;
  const extra = card.querySelector('[data-field="auth_extra"]').value.trim();
  return {
    protocol_family: card.querySelector('[data-field="protocol_family"]').value,
    base_url: card.querySelector('[data-field="base_url"]').value.trim(),
    auth_strategy: authStrategyFromFields(kind, extra),
    key_pool_id: card.querySelector('[data-field="key_pool_id"]').value.trim(),
    request_timeout_ms: (() => {
      const raw = card.querySelector('[data-field="request_timeout_ms"]').value.trim();
      return raw === "" ? null : Number(raw);
    })(),
    extra_headers: collectStringMapRows(card.querySelector('[data-map-group="extra_headers"]')),
    extra_query: collectStringMapRows(card.querySelector('[data-map-group="extra_query"]')),
    enabled: card.querySelector('[data-field="enabled"]').checked,
  };
}

function renderModelForm(model) {
  $("modelId").value = model.id || "";
  $("modelDisplayName").value = model.display_name || "";
  $("modelUpstreamModel").value = model.upstream_model || "";
  $("modelDescription").value = model.description || "";
  $("modelEnabled").checked = model.enabled !== false;
  $("modelMetadataPairs").innerHTML = renderMetadataRowsMarkup(model.metadata || {});
  renderModelEndpoints(model.endpoints || []);
}

function renderModelEndpoints(endpoints) {
  const root = $("modelEndpoints");
  if (!endpoints.length) {
    root.innerHTML = `<div class="resource-empty">当前还没有端点，点击“新增端点”开始配置。</div>`;
    return;
  }

  root.innerHTML = endpoints
    .map((endpoint, index) => renderEndpointEditorMarkup(endpoint, index, "remove-model-endpoint"))
    .join("");

  root.querySelectorAll('[data-field="auth_kind"]').forEach((select) => {
    select.addEventListener("change", () => {
      const card = select.closest(".subitem-card");
      const kind = select.value;
      const label = authKinds.find((item) => item.value === kind)?.extraLabel || "附加参数";
      card.querySelector(".auth-extra-label").textContent = label || "附加参数";
    });
  });
}

function collectModelForm() {
  const endpoints = Array.from($("modelEndpoints").querySelectorAll(".endpoint-card")).map((card, index) =>
    collectEndpointCard(card, `模型端点 ${index + 1}`)
  );

  return {
    id: $("modelId").value.trim(),
    display_name: $("modelDisplayName").value.trim(),
    upstream_model: $("modelUpstreamModel").value.trim(),
    description: $("modelDescription").value,
    enabled: $("modelEnabled").checked,
    endpoints,
    metadata: collectMetadataRows($("modelMetadataPairs"), "模型 metadata"),
  };
}

function renderGroupForm(group) {
  $("groupId").value = group.id || "";
  $("groupDisplayName").value = group.display_name || "";
  $("groupFallbackRatio").value = group.fallback_ratio ?? "";
  $("groupDecayFactor").value = group.decay_factor ?? "";
  $("groupPenaltyRate").value = group.penalty_rate ?? "";
  $("groupRecoveryRate").value = group.recovery_rate ?? "";
  $("groupRaceMaxWaitTimeMs").value = group.race_max_wait_time_ms ?? "";
  $("groupEnabled").checked = group.enabled !== false;
  renderGroupCandidates(group.candidates || [], group.id || "group-example");
}

function renderGroupCandidates(candidates, groupId) {
  const root = $("groupCandidates");
  if (!candidates.length) {
    root.innerHTML = `<div class="resource-empty">当前还没有候选，点击“新增候选”开始配置。</div>`;
    return;
  }

  root.innerHTML = candidates
    .map((candidate, index) => {
      const effectiveUpstreamModel = candidateResolvedUpstreamModel(candidate);
      return `
      <article class="subitem-card" data-candidate-index="${index}">
        <div class="subitem-head">
          <div>
            <h5>候选 ${index + 1}</h5>
            <p class="resource-item-subtitle">${escapeHtml(candidate.name || candidate.id || "未命名候选")}</p>
          </div>
          <button type="button" data-action="remove-group-candidate" data-index="${index}" class="danger">删除候选</button>
        </div>
        <div class="subitem-grid candidate-main-grid">
          <label class="field">
            <span>候选名称</span>
            <input data-field="name" value="${escapeHtml(candidate.name || "")}" />
          </label>
          <label class="field">
            <span>选择模型</span>
            <select data-field="model_id">${renderModelOptions(candidate.model_id || "", { allowEmpty: true })}</select>
          </label>
          <label class="field">
            <span>上游模型名</span>
            <input data-field="upstream_model" value="${escapeHtml(effectiveUpstreamModel)}" placeholder="例如：z-ai/glm4.7" />
            <small class="field-help">可以直接手填；如果上面选了模型，这里会自动带出真实上游模型名。</small>
          </label>
          <label class="field">
            <span>初始权重</span>
            <input data-field="initial_weight" type="number" step="0.01" value="${escapeHtml(candidate.initial_weight ?? "")}" />
          </label>
          <label class="field">
            <span>响应保护时间（毫秒）</span>
            <input data-field="response_protection_timeout_ms" type="number" min="1" value="${escapeHtml(candidate.response_protection_timeout_ms ?? "")}" />
          </label>
        </div>
        <details class="advanced-details">
          <summary>高级配置</summary>
          <div class="advanced-details-body">
            <div class="subitem-grid three-cols">
              <label class="field">
                <span>候选 ID</span>
                <input data-field="id" value="${escapeHtml(candidate.id || "")}" placeholder="留空将自动生成" />
              </label>
              <label class="field checkbox-field">
                <span>启用状态</span>
                <label class="checkbox-inline">
                  <input data-field="enabled" type="checkbox" ${candidate.enabled !== false ? "checked" : ""} />
                  <span>已启用</span>
                </label>
              </label>
            </div>
            <div class="subitem-grid two-cols">
              <div class="field">
                <div class="inline-head">
                  <span>扩展字段</span>
                  <button type="button" data-action="add-candidate-metadata-row">新增字段</button>
                </div>
                <div class="kv-editor" data-meta-root="candidate-metadata">
                  ${renderMetadataRowsMarkup(candidate.metadata || {})}
                </div>
              </div>
              <div class="field">
                <div class="inline-head">
                  <span>覆盖端点</span>
                  <button type="button" data-action="add-inline-endpoint">新增覆盖端点</button>
                </div>
                <div class="nested-endpoint-list" data-inline-endpoints-root>
                  ${(candidate.inline_endpoint_overrides || []).length
                    ? (candidate.inline_endpoint_overrides || []).map((endpoint, overrideIndex) => renderEndpointEditorMarkup(endpoint, overrideIndex, "remove-inline-endpoint", "覆盖端点")).join("")
                    : '<div class="resource-empty">当前没有覆盖端点，未配置时将沿用模型配置里的协议端点。</div>'}
                </div>
              </div>
            </div>
          </div>
        </details>
      </article>
    `;
    })
    .join("");
}

function collectGroupForm() {
  const id = $("groupId").value.trim();
  const candidates = Array.from($("groupCandidates").querySelectorAll(".subitem-card")).map((card, index) => ({
    id: (() => {
      const raw = card.querySelector('[data-field="id"]').value.trim();
      const name = card.querySelector('[data-field="name"]').value.trim();
      return raw || generateCandidateId(id || "group", name, index);
    })(),
    group_id: id,
    name: card.querySelector('[data-field="name"]').value.trim(),
    model_id: card.querySelector('[data-field="model_id"]').value.trim() || null,
    upstream_model: card.querySelector('[data-field="upstream_model"]').value.trim(),
    inline_endpoint_overrides: Array.from(card.querySelectorAll('[data-inline-endpoints-root] > .endpoint-card')).map((endpointCard, endpointIndex) =>
      collectEndpointCard(endpointCard, `候选 ${index + 1} 覆盖端点 ${endpointIndex + 1}`)
    ),
    initial_weight: Number(card.querySelector('[data-field="initial_weight"]').value || "0"),
    response_protection_timeout_ms: Number(card.querySelector('[data-field="response_protection_timeout_ms"]').value || "0"),
    enabled: card.querySelector('[data-field="enabled"]').checked,
    metadata: collectMetadataRows(card.querySelector('[data-meta-root="candidate-metadata"]'), `候选 ${index + 1} metadata`),
  }));

  return {
    id,
    display_name: $("groupDisplayName").value.trim(),
    fallback_ratio: numberValue("groupFallbackRatio"),
    decay_factor: numberValue("groupDecayFactor"),
    penalty_rate: numberValue("groupPenaltyRate"),
    recovery_rate: numberValue("groupRecoveryRate"),
    race_max_wait_time_ms: (() => {
      const raw = $("groupRaceMaxWaitTimeMs").value.trim();
      return raw === "" ? null : Number(raw);
    })(),
    enabled: $("groupEnabled").checked,
    candidates,
  };
}

function updateKeyPoolAuthExtraLabel() {
  const kind = $("keyPoolAuthKind").value;
  const label = authKinds.find((item) => item.value === kind)?.extraLabel || "附加参数";
  $("keyPoolAuthExtraLabel").textContent = label || "附加参数";
}

function renderKeyPoolForm(pool) {
  $("keyPoolId").value = pool.id || "";
  $("keyPoolDisplayName").value = pool.display_name || "";
  $("keyPoolEnabled").checked = pool.enabled !== false;
  $("keyPoolSelectionStrategy").value = pool.selection_strategy || "random";
  $("keyPoolAuthKind").innerHTML = renderAuthKindOptions(authStrategyToFields(pool.auth_strategy).kind);
  $("keyPoolAuthExtra").value = authStrategyToFields(pool.auth_strategy).extra || "";
  updateKeyPoolAuthExtraLabel();
  renderKeyPoolKeys(pool.keys || [], pool.id || "keypool-example");
}

function renderKeyPoolKeys(keys) {
  const root = $("keyPoolKeys");
  if (!keys.length) {
    root.innerHTML = `<div class="resource-empty">当前还没有 Key，点击“新增 Key”开始配置。</div>`;
    return;
  }

  root.innerHTML = keys
    .map((key, index) => `
      <article class="subitem-card" data-key-index="${index}">
        <div class="subitem-head">
          <div>
            <h5>Key ${index + 1}</h5>
            <p class="resource-item-subtitle">${escapeHtml(key.id || "未命名 Key")}</p>
          </div>
          <button type="button" data-action="remove-key-pool-key" data-index="${index}" class="danger">删除 Key</button>
        </div>
        <div class="subitem-grid three-cols">
          <label class="field">
            <span>Key ID</span>
            <input data-field="id" value="${escapeHtml(key.id || "")}" />
          </label>
          <label class="field">
            <span>密钥值</span>
            <input data-field="secret" value="${escapeHtml(key.secret || "")}" />
          </label>
          <label class="field checkbox-field">
            <span>启用状态</span>
            <label class="checkbox-inline">
              <input data-field="enabled" type="checkbox" ${key.enabled !== false ? "checked" : ""} />
              <span>已启用</span>
            </label>
          </label>
        </div>
        <label class="field">
          <div class="inline-head">
            <span>扩展字段</span>
            <button type="button" data-action="add-key-metadata-row">新增字段</button>
          </div>
          <div class="kv-editor" data-meta-root="key-metadata">
            ${renderMetadataRowsMarkup(key.metadata || {})}
          </div>
        </label>
      </article>
    `)
    .join("");
}

function collectKeyPoolForm() {
  const id = $("keyPoolId").value.trim();
  const keys = Array.from($("keyPoolKeys").querySelectorAll(".subitem-card")).map((card) => ({
    id: card.querySelector('[data-field="id"]').value.trim(),
    key_pool_id: id,
    secret: card.querySelector('[data-field="secret"]').value,
    enabled: card.querySelector('[data-field="enabled"]').checked,
    metadata: collectMetadataRows(card.querySelector('[data-meta-root="key-metadata"]'), "Key metadata"),
  }));

  return {
    id,
    display_name: $("keyPoolDisplayName").value.trim(),
    auth_strategy: authStrategyFromFields($("keyPoolAuthKind").value, $("keyPoolAuthExtra").value.trim()),
    selection_strategy: $("keyPoolSelectionStrategy").value,
    enabled: $("keyPoolEnabled").checked,
    keys,
  };
}

function renderResourceList(name, items) {
  const config = resourceConfigs[name];
  const root = $(config.listId);
  const query = normalizedSearchValue(config.searchId);
  const selectedId = state.selected[name];

  const filtered = items.filter((item) => {
    const haystack = [
      item.id,
      item.display_name,
      item.upstream_model,
      ...(item.candidate_names || []),
      ...(item.protocol_families || []),
    ].join(" ");
    return matchesSearch(haystack, query);
  });

  if (!filtered.length) {
    root.innerHTML = `<div class="resource-empty">${items.length ? `没有匹配当前搜索条件的${config.summaryLabel}。` : `当前还没有${config.summaryLabel}。`}</div>`;
    return;
  }

  root.innerHTML = filtered
    .map((item) => {
      const active = item.id === selectedId ? "active" : "";
      const enabledText = item.enabled === false ? badge("已禁用", "danger") : badge("已启用", "ok");
      const extras = [];
      if (item.upstream_model) extras.push(badge(item.upstream_model));
      if (item.protocol_families?.length) extras.push(...item.protocol_families.map((protocol) => badge(formatProtocol(protocol))));
      if (item.key_pool_ids?.length) extras.push(...item.key_pool_ids.map((keyPoolId) => badge(`Key 池 ${keyPoolId}`)));
      if (item.total_keys !== undefined) extras.push(badge(`Key ${item.enabled_keys}/${item.total_keys}`));
      if (item.candidate_count !== undefined) extras.push(badge(`候选 ${item.enabled_candidate_count}/${item.candidate_count}`));

      const note = name === "groups"
        ? (item.candidate_names?.length ? item.candidate_names.join(" / ") : "暂无候选")
        : "";

      return `
        <article class="resource-item ${active}" data-action="select-resource" data-name="${name}" data-id="${escapeHtml(item.id)}">
          <div class="resource-item-head">
            <div>
              <h4 class="resource-item-title">${escapeHtml(item.display_name || item.id)}</h4>
              <p class="resource-item-subtitle">${escapeHtml(item.id)}</p>
            </div>
          </div>
          <div class="resource-item-meta">
            ${enabledText}
            ${extras.join("")}
          </div>
          ${note ? `<p class="resource-item-note">${escapeHtml(note)}</p>` : ""}
        </article>
      `;
    })
    .join("");
}

async function loadCollection(name) {
  const config = resourceConfigs[name];
  const items = await fetchJson(`/admin/${config.path}`);
  if (name === "models") state.models = items;
  if (name === "groups") state.groups = items;
  if (name === "key-pools") state.keyPools = items;
  refreshDependentForms(name);
  renderResourceList(name, items);
  refreshOverviewCards();
}

function refreshDependentForms(changedResource) {
  try {
    if (changedResource === "key-pools") {
      renderModelForm(collectModelForm());
      renderGroupForm(collectGroupForm());
    }
    if (changedResource === "models") {
      renderGroupForm(collectGroupForm());
    }
  } catch (error) {
    console.warn("refreshDependentForms skipped", error);
  }
}

async function loadResource(name, id) {
  const config = resourceConfigs[name];
  if (!id) {
    setFlash(`缺少${config.summaryLabel} ID`, "error");
    return;
  }
  clearIssues(name);
  const payload = await fetchJson(`/admin/${config.path}/${encodeURIComponent(id)}`);
  state.selected[name] = id;
  if (name === "models") renderModelForm(payload);
  if (name === "groups") renderGroupForm(payload);
  if (name === "key-pools") renderKeyPoolForm(payload);
  renderResourceList(name, name === "models" ? state.models : name === "groups" ? state.groups : state.keyPools);
  setFlash(`已加载${config.summaryLabel} ${id}`);
}

function prepareNewResource(name) {
  const config = resourceConfigs[name];
  const idSeed = `${name.replace(/[^a-z]/g, "")}-${Date.now().toString().slice(-6)}`;
  const payload = config.template(idSeed);
  state.selected[name] = "";
  clearIssues(name);
  if (name === "models") renderModelForm(payload);
  if (name === "groups") renderGroupForm(payload);
  if (name === "key-pools") renderKeyPoolForm(payload);
  renderResourceList(name, name === "models" ? state.models : name === "groups" ? state.groups : state.keyPools);
  setFlash(`已创建${config.summaryLabel}表单草稿`);
}

function collectResource(name) {
  if (name === "models") return collectModelForm();
  if (name === "groups") return collectGroupForm();
  if (name === "key-pools") return collectKeyPoolForm();
  throw new Error(`unsupported resource ${name}`);
}

async function saveResource(name) {
  const config = resourceConfigs[name];
  try {
    clearIssues(name);
    const payload = collectResource(name);
    if (!payload.id) {
      setFlash(`请先填写${config.summaryLabel} ID`, "error");
      return;
    }
    const targetId = state.selected[name] || payload.id;
    await fetchJson(`/admin/${config.path}/${encodeURIComponent(targetId)}`, {
      method: "PUT",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(payload),
    });
    state.selected[name] = payload.id;
    await loadCollection(name);
    await loadResource(name, payload.id);
    if (name === "groups") await refreshRuntime();
    setFlash(`已保存${config.summaryLabel} ${payload.id}`);
  } catch (error) {
    if (error.payload?.issues) renderIssues(name, error.payload);
    setFlash(error.message || `保存${config.summaryLabel}失败`, "error");
  }
}

async function validateResource(name) {
  const config = resourceConfigs[name];
  try {
    const payload = collectResource(name);
    const result = await fetchJson(`/admin/validate/${config.validatePath}`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(payload),
    });
    renderIssues(name, result);
    setFlash(result.valid ? `${config.summaryLabel}校验通过` : `校验返回 ${result.issues.length} 个问题`);
  } catch (error) {
    setFlash(error.message || `校验${config.summaryLabel}失败`, "error");
  }
}

async function deleteResource(name) {
  const config = resourceConfigs[name];
  const payload = collectResource(name);
  if (!payload.id) {
    setFlash(`缺少${config.summaryLabel} ID`, "error");
    return;
  }
  if (!confirm(`确认删除${config.summaryLabel}“${payload.id}”吗？`)) return;
  await fetchJson(`/admin/${config.path}/${encodeURIComponent(payload.id)}`, { method: "DELETE" });
  state.selected[name] = "";
  prepareNewResource(name);
  await loadCollection(name);
  if (name === "groups") await refreshRuntime();
  setFlash(`已删除${config.summaryLabel} ${payload.id}`);
}

function addModelEndpoint() {
  const model = collectModelForm();
  model.endpoints.push(defaultEndpoint());
  renderModelForm(model);
}

function removeModelEndpoint(index) {
  const model = collectModelForm();
  model.endpoints.splice(index, 1);
  renderModelForm(model);
}

function addGroupCandidate() {
  const group = collectGroupForm();
  group.candidates.push(defaultCandidate(group.id || "group-example"));
  renderGroupForm(group);
}

function removeGroupCandidate(index) {
  const group = collectGroupForm();
  group.candidates.splice(index, 1);
  renderGroupForm(group);
}

function appendInlineEndpoint(root, endpoint = defaultEndpoint()) {
  const empty = root.querySelector(".resource-empty");
  if (empty) empty.remove();
  const nextIndex = root.querySelectorAll(".endpoint-card").length;
  root.insertAdjacentHTML("beforeend", renderEndpointEditorMarkup(endpoint, nextIndex, "remove-inline-endpoint", "覆盖端点"));
}

function addKeyPoolKey() {
  const pool = collectKeyPoolForm();
  pool.keys.push(defaultKey(pool.id || "keypool-example"));
  renderKeyPoolForm(pool);
}

function removeKeyPoolKey(index) {
  const pool = collectKeyPoolForm();
  pool.keys.splice(index, 1);
  renderKeyPoolForm(pool);
}

function computeDiagnostics(runtimeGroups) {
  return runtimeGroups.flatMap((group) =>
    (group.race_stats?.recent_races || [])
      .filter((race) => race.errors && Object.keys(race.errors).length > 0)
      .map((race) => ({
        group_id: group.group_id,
        display_name: group.display_name,
        protocol: race.protocol,
        winner: race.winner,
        duration_ms: race.duration_ms,
        timestamp: race.timestamp,
        errors: race.errors,
      }))
  );
}

function refreshOverviewCards() {
  $("overviewModels").textContent = `已配置 ${state.models.length} 个`;
  $("overviewKeyPools").textContent = `已配置 ${state.keyPools.length} 个`;
  $("overviewDiagnostics").textContent = `最近异常 ${computeDiagnostics(state.runtime).length} 条`;
  $("overviewMetrics").href = "/admin/metrics";
}

function renderRuntime(runtimeGroups) {
  const topCards = $("runtimeTopCards");
  const root = $("runtimeCards");
  const query = normalizedSearchValue("runtimeSearch");
  const filteredGroups = runtimeGroups.filter((group) => {
    const haystack = [
      group.group_id,
      group.display_name,
      ...(group.candidate_statuses || []).map((item) => item.candidate_name),
      ...Object.keys(group.eligible_candidate_counts_by_protocol || {}),
    ].join(" ");
    return matchesSearch(haystack, query);
  });

  const totalRaces = runtimeGroups.reduce((sum, group) => sum + (group.race_stats?.total_races || 0), 0);
  const totalCandidates = runtimeGroups.reduce((sum, group) => sum + (group.candidate_statuses?.length || 0), 0);
  const recovering = runtimeGroups.flatMap((group) => group.candidate_statuses || []).filter((item) => item.status === "recovering").length;

  topCards.innerHTML = [
    { title: "竞速组", value: runtimeGroups.length },
    { title: "累计竞速", value: totalRaces },
    { title: "恢复中候选", value: `${recovering}/${totalCandidates}` },
  ]
    .map((item) => `<article class="metric-card"><span class="metric-label">${item.title}</span><strong>${item.value}</strong></article>`)
    .join("");

  if (!filteredGroups.length) {
    root.innerHTML = `<div class="resource-empty">${runtimeGroups.length ? "没有匹配当前搜索条件的运行时竞速组。" : "当前还没有运行时竞速组。"}</div>`;
    return;
  }

  root.innerHTML = filteredGroups
    .map((group) => {
      const protocols = Object.entries(group.eligible_candidate_counts_by_protocol || {})
        .map(([protocol, count]) => badge(`${formatProtocol(protocol)} ${count}`))
        .join("");
      const maxWeight = Math.max(1, ...(group.candidate_statuses || []).map((item) => Math.max(item.effective_weight, item.initial_weight)));
      const candidates = (group.candidate_statuses || [])
        .map((candidate) => {
          const width = Math.max(4, Math.round((candidate.effective_weight / maxWeight) * 100));
          const protocolText = (candidate.eligible_protocol_families || []).map(formatProtocol).join(" / ") || "无";
          return `
            <div class="progress-row">
              <div class="progress-label">${escapeHtml(candidate.candidate_name)}</div>
              <div class="progress-track"><span style="width:${width}%"></span></div>
              <div class="progress-value">${candidate.effective_weight.toFixed(2)} / ${candidate.initial_weight.toFixed(2)}</div>
            </div>
            <div class="badge-row">
              ${badge(formatStatus(candidate.status), statusBadgeClass(true, candidate.status))}
              ${badge(protocolText)}
            </div>
          `;
        })
        .join("") || `<div class="resource-empty">暂无候选</div>`;

      const winners = Object.entries(group.race_stats?.winner_distribution || {})
        .map(([winner, count]) => {
          const maxWinnerCount = Math.max(1, ...Object.values(group.race_stats?.winner_distribution || {}));
          const width = Math.max(4, Math.round((count / maxWinnerCount) * 100));
          return `
            <div class="progress-row">
              <div class="progress-label">${escapeHtml(winner)}</div>
              <div class="progress-track"><span style="width:${width}%"></span></div>
              <div class="progress-value">${count}</div>
            </div>
          `;
        })
        .join("") || `<div class="resource-empty">暂无胜出记录</div>`;

      const recentRaces = (group.race_stats?.recent_races || [])
        .slice(0, 4)
        .map((race) => `<li>${escapeHtml(formatProtocol(race.protocol))} · 胜出 ${escapeHtml(race.winner || "全部失败")} · ${escapeHtml(race.duration_ms)} ms</li>`)
        .join("") || "<li>暂无竞速记录</li>";

      return `
        <article class="runtime-card">
          <div class="runtime-head">
            <div>
              <h3>${escapeHtml(group.display_name)}</h3>
              <p class="muted">${escapeHtml(group.group_id)}</p>
            </div>
            <div class="badge-row">
              ${badge(group.enabled ? "已启用" : "已禁用", statusBadgeClass(group.enabled))}
              ${protocols}
            </div>
          </div>
          <div class="runtime-columns">
            <section class="runtime-section">
              <h4>当前权重</h4>
              <div class="progress-stack">${candidates}</div>
            </section>
            <section class="runtime-section">
              <h4>赢家分布</h4>
              <div class="progress-stack">${winners}</div>
            </section>
            <section class="runtime-section">
              <h4>最近竞速</h4>
              <ul class="runtime-list">${recentRaces}</ul>
            </section>
          </div>
        </article>
      `;
    })
    .join("");
}

function renderDiagnostics() {
  const root = $("diagnosticsList");
  const query = normalizedSearchValue("diagnosticSearch");
  const groupFilter = $("diagnosticGroupFilter").value;
  const protocolFilter = $("diagnosticProtocolFilter").value;
  const diagnostics = computeDiagnostics(state.runtime)
    .filter((item) => !groupFilter || item.group_id === groupFilter)
    .filter((item) => !protocolFilter || item.protocol === protocolFilter)
    .filter((item) => matchesSearch([item.group_id, item.display_name, item.protocol, item.winner, JSON.stringify(item.errors || {})].join(" "), query));

  if (!diagnostics.length) {
    root.innerHTML = `<div class="diagnostic-empty">没有匹配当前筛选条件的诊断记录。</div>`;
    return;
  }

  root.innerHTML = diagnostics
    .map((item) => {
      const errors = Object.entries(item.errors || {}).map(([candidate, reason]) => `<li>${escapeHtml(candidate)}：${escapeHtml(reason)}</li>`).join("");
      return `
        <article class="diagnostic-card">
          <div class="diagnostic-head">
            <div>
              <h3>${escapeHtml(item.display_name)}</h3>
              <p class="muted">${escapeHtml(item.group_id)} · ${escapeHtml(formatProtocol(item.protocol))} · ${escapeHtml(item.timestamp || "")}</p>
            </div>
            <div class="badge-row">
              ${badge(item.winner || "全部失败", item.winner ? "warn" : "danger")}
              ${badge(`${item.duration_ms} ms`)}
            </div>
          </div>
          <ul class="diagnostic-errors">${errors}</ul>
        </article>
      `;
    })
    .join("");
}

function refreshDiagnosticFilters() {
  const groupSelect = $("diagnosticGroupFilter");
  const protocolSelect = $("diagnosticProtocolFilter");
  const selectedGroup = groupSelect.value;
  const selectedProtocol = protocolSelect.value;
  const groups = Array.from(new Set(state.runtime.map((item) => item.group_id)));
  const protocols = Array.from(new Set(computeDiagnostics(state.runtime).map((item) => item.protocol)));

  groupSelect.innerHTML = `<option value="">全部竞速组</option>${groups.map((group) => `<option value="${group}">${group}</option>`).join("")}`;
  protocolSelect.innerHTML = `<option value="">全部协议</option>${protocols.map((protocol) => `<option value="${protocol}">${formatProtocol(protocol)}</option>`).join("")}`;
  groupSelect.value = groups.includes(selectedGroup) ? selectedGroup : "";
  protocolSelect.value = protocols.includes(selectedProtocol) ? selectedProtocol : "";
}

async function refreshRuntime() {
  state.runtime = await fetchJson("/admin/runtime/groups");
  $("runtimeSummary").textContent = `${state.runtime.length} 个`;
  renderRuntime(state.runtime);
  refreshDiagnosticFilters();
  renderDiagnostics();
  refreshOverviewCards();
}

async function refreshOverview() {
  state.health = await fetchJson("/admin/healthz");
  $("proxyHealth").textContent = state.health.proxy_bind_addr;
  $("adminHealth").textContent = `${state.health.status === "ok" ? "正常" : state.health.status} @ ${state.health.admin_bind_addr}`;
}

function renderSettings(settings) {
  $("settingDiagnostics").checked = Boolean(settings.enable_race_diagnostics_header);
  $("settingMaxBufferEvents").value = String(settings.max_buffer_events || 1);
  $("settingBufferTimeoutMs").value = String(settings.buffer_backpressure_timeout_ms || 1);
  $("settingsPreview").textContent = pretty(settings);
}

async function refreshSettings() {
  state.settings = await fetchJson("/admin/settings");
  renderSettings(state.settings);
}

async function saveSettings() {
  const payload = {
    enable_race_diagnostics_header: $("settingDiagnostics").checked,
    max_buffer_events: Number($("settingMaxBufferEvents").value || "1"),
    buffer_backpressure_timeout_ms: Number($("settingBufferTimeoutMs").value || "1"),
  };
  state.settings = await fetchJson("/admin/settings", {
    method: "PUT",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(payload),
  });
  renderSettings(state.settings);
  setFlash("设置已保存");
}

function runAction(fn) {
  return () => Promise.resolve().then(fn).catch((error) => {
    console.error(error);
    setFlash(error.message || String(error), "error");
  });
}

function bindEvents() {
  document.querySelectorAll("[data-tab]").forEach((button) => {
    button.addEventListener("click", () => switchTab(button.dataset.tab));
  });

  $("refreshModels").addEventListener("click", runAction(() => loadCollection("models")));
  $("newModel").addEventListener("click", () => prepareNewResource("models"));
  $("saveModel").addEventListener("click", runAction(() => saveResource("models")));
  $("validateModel").addEventListener("click", runAction(() => validateResource("models")));
  $("deleteModel").addEventListener("click", runAction(() => deleteResource("models")));
  $("addModelEndpoint").addEventListener("click", runAction(addModelEndpoint));
  $("addModelMetadataRow").addEventListener("click", () => appendMetadataRow($("modelMetadataPairs")));
  $("modelSearch").addEventListener("input", () => renderResourceList("models", state.models));

  $("refreshGroups").addEventListener("click", runAction(() => loadCollection("groups")));
  $("newGroup").addEventListener("click", () => prepareNewResource("groups"));
  $("saveGroup").addEventListener("click", runAction(() => saveResource("groups")));
  $("validateGroup").addEventListener("click", runAction(() => validateResource("groups")));
  $("deleteGroup").addEventListener("click", runAction(() => deleteResource("groups")));
  $("addGroupCandidate").addEventListener("click", runAction(addGroupCandidate));
  $("groupSearch").addEventListener("input", () => renderResourceList("groups", state.groups));

  $("refreshKeyPools").addEventListener("click", runAction(() => loadCollection("key-pools")));
  $("newKeyPool").addEventListener("click", () => prepareNewResource("key-pools"));
  $("saveKeyPool").addEventListener("click", runAction(() => saveResource("key-pools")));
  $("validateKeyPool").addEventListener("click", runAction(() => validateResource("key-pools")));
  $("deleteKeyPool").addEventListener("click", runAction(() => deleteResource("key-pools")));
  $("addKeyPoolKey").addEventListener("click", runAction(addKeyPoolKey));
  $("keyPoolSearch").addEventListener("input", () => renderResourceList("key-pools", state.keyPools));
  $("keyPoolAuthKind").addEventListener("change", updateKeyPoolAuthExtraLabel);

  $("refreshRuntime").addEventListener("click", runAction(refreshRuntime));
  $("runtimeSearch").addEventListener("input", () => renderRuntime(state.runtime));
  $("refreshDiagnostics").addEventListener("click", runAction(refreshRuntime));
  $("diagnosticSearch").addEventListener("input", renderDiagnostics);
  $("diagnosticGroupFilter").addEventListener("change", renderDiagnostics);
  $("diagnosticProtocolFilter").addEventListener("change", renderDiagnostics);

  $("refreshSettings").addEventListener("click", runAction(refreshSettings));
  $("saveSettings").addEventListener("click", runAction(saveSettings));

  document.body.addEventListener("click", (event) => {
    const selectCard = event.target.closest('[data-action="select-resource"]');
    if (selectCard) {
      const { name, id } = selectCard.dataset;
      runAction(() => loadResource(name, id))();
      return;
    }

    const removeEndpoint = event.target.closest('[data-action="remove-model-endpoint"]');
    if (removeEndpoint) {
      runAction(() => removeModelEndpoint(Number(removeEndpoint.dataset.index)))();
      return;
    }

    const removeCandidate = event.target.closest('[data-action="remove-group-candidate"]');
    if (removeCandidate) {
      runAction(() => removeGroupCandidate(Number(removeCandidate.dataset.index)))();
      return;
    }

    const removeKey = event.target.closest('[data-action="remove-key-pool-key"]');
    if (removeKey) {
      runAction(() => removeKeyPoolKey(Number(removeKey.dataset.index)))();
      return;
    }

    const addMapRow = event.target.closest('[data-action="add-map-row"]');
    if (addMapRow) {
      const card = addMapRow.closest(".endpoint-card");
      const target = addMapRow.dataset.target;
      const root = card?.querySelector(`[data-map-group="${target}"]`);
      if (root) {
        appendStringMapRow(root, target === "extra_headers" ? "Header 名称" : "参数名", target === "extra_headers" ? "Header 值" : "参数值");
      }
      return;
    }

    const removeMapRow = event.target.closest('[data-action="remove-map-row"]');
    if (removeMapRow) {
      const row = removeMapRow.closest(".map-row");
      const root = removeMapRow.closest(".kv-editor");
      row?.remove();
      if (root && !root.querySelector(".map-row")) {
        root.innerHTML = `<div class="resource-empty">当前没有配置项，点击“新增”添加。</div>`;
      }
      return;
    }

    const addCandidateMetadata = event.target.closest('[data-action="add-candidate-metadata-row"]');
    if (addCandidateMetadata) {
      const root = addCandidateMetadata.closest(".field")?.querySelector('[data-meta-root="candidate-metadata"]');
      if (root) appendMetadataRow(root);
      return;
    }

    const addKeyMetadata = event.target.closest('[data-action="add-key-metadata-row"]');
    if (addKeyMetadata) {
      const root = addKeyMetadata.closest(".field")?.querySelector('[data-meta-root="key-metadata"]');
      if (root) appendMetadataRow(root);
      return;
    }

    const removeMetadataRow = event.target.closest('[data-action="remove-metadata-row"]');
    if (removeMetadataRow) {
      const row = removeMetadataRow.closest(".metadata-row");
      const root = removeMetadataRow.closest(".kv-editor");
      row?.remove();
      if (root && !root.querySelector(".metadata-row")) {
        root.innerHTML = `<div class="resource-empty">当前没有 metadata 字段，点击“新增字段”添加。</div>`;
      }
      return;
    }

    const addInlineEndpoint = event.target.closest('[data-action="add-inline-endpoint"]');
    if (addInlineEndpoint) {
      const root = addInlineEndpoint.closest(".field")?.querySelector("[data-inline-endpoints-root]");
      if (root) appendInlineEndpoint(root);
      return;
    }

    const removeInlineEndpoint = event.target.closest('[data-action="remove-inline-endpoint"]');
    if (removeInlineEndpoint) {
      const card = removeInlineEndpoint.closest(".endpoint-card");
      const root = removeInlineEndpoint.closest("[data-inline-endpoints-root]");
      card?.remove();
      if (root && !root.querySelector(".endpoint-card")) {
        root.innerHTML = '<div class="resource-empty">当前没有覆盖端点，未配置时将沿用模型端点。</div>';
      }
    }
  });

  document.body.addEventListener("change", (event) => {
    const select = event.target.closest('[data-field="auth_kind"]');
    if (select) {
      const card = select.closest(".endpoint-card");
      const label = authKinds.find((item) => item.value === select.value)?.extraLabel || "附加参数";
      const labelNode = card?.querySelector(".auth-extra-label");
      if (labelNode) labelNode.textContent = label || "附加参数";
      return;
    }

    const modelSelect = event.target.closest('.subitem-card [data-field="model_id"]');
    if (modelSelect) {
      const card = modelSelect.closest(".subitem-card");
      const upstreamInput = card?.querySelector('[data-field="upstream_model"]');
      const model = modelById(modelSelect.value);
      if (upstreamInput && model?.upstream_model) {
        upstreamInput.value = model.upstream_model;
      }
    }
  });
}

async function boot() {
  $("keyPoolAuthKind").innerHTML = renderAuthKindOptions("bearer");
  bindEvents();
  prepareNewResource("models");
  prepareNewResource("groups");
  prepareNewResource("key-pools");
  await Promise.all([
    refreshOverview(),
    loadCollection("models"),
    loadCollection("groups"),
    loadCollection("key-pools"),
    refreshRuntime(),
    refreshSettings(),
  ]);
}

boot().catch((error) => {
  console.error(error);
  setFlash(error.message || String(error), "error");
});
