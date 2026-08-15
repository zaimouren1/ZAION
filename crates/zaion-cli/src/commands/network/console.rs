//! Embedded WebUI HTML served at `/ui`.
//!
//! The browser control plane is intentionally dependency-free: it is a single
//! HTML document backed by the local gateway routes.

/// Return the full HTML document for the Zaion gateway WebUI.
pub(super) fn web_console_html() -> String {
    [WEBUI_HEAD, WEBUI_BODY, WEBUI_SCRIPT].concat()
}

const WEBUI_HEAD: &str = r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>Zaion 母舰控制台</title>
<style>
  *, *::before, *::after { box-sizing: border-box; }
  :root {
    color-scheme: light;
    --page: #f5f7f8;
    --surface: #ffffff;
    --surface-soft: #eef3f4;
    --ink: #171b1f;
    --muted: #667178;
    --line: #d7e0e3;
    --line-strong: #aebdc2;
    --accent: #1d6f79;
    --accent-soft: #d9eef0;
    --warm: #9d5c4d;
    --ok: #0f7a4f;
    --warn: #9a6500;
    --bad: #b03a2e;
    --shadow: 0 18px 42px rgba(24, 34, 38, 0.10);
    --mono: "SFMono-Regular", Consolas, "Liberation Mono", monospace;
    --sans: Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
  }
  html, body {
    margin: 0;
    min-height: 100vh;
    background: var(--page);
    color: var(--ink);
    font-family: var(--sans);
    font-size: 14px;
    line-height: 1.45;
  }
  button, input, textarea {
    font: inherit;
  }
  button {
    min-height: 34px;
    border: 1px solid var(--line-strong);
    border-radius: 7px;
    background: var(--surface);
    color: var(--ink);
    padding: 6px 10px;
    cursor: pointer;
    transition: background 120ms ease, border-color 120ms ease, transform 120ms ease;
  }
  button:hover { background: var(--surface-soft); border-color: var(--accent); }
  button:active { transform: translateY(1px); }
  button.primary {
    background: var(--accent);
    border-color: var(--accent);
    color: white;
  }
  button.danger { color: var(--bad); border-color: rgba(176, 58, 46, 0.35); }
  input, textarea {
    width: 100%;
    border: 1px solid var(--line);
    border-radius: 7px;
    background: var(--surface);
    color: var(--ink);
    padding: 8px 10px;
    min-height: 36px;
  }
  textarea {
    min-height: 74px;
    resize: vertical;
  }
  input:focus, textarea:focus, button:focus {
    outline: 2px solid rgba(29, 111, 121, 0.22);
    outline-offset: 1px;
    border-color: var(--accent);
  }
  .workspace-shell {
    min-height: 100vh;
    display: grid;
    grid-template-columns: 258px minmax(0, 1fr);
  }
  .rail {
    position: sticky;
    top: 0;
    height: 100vh;
    padding: 22px 18px;
    border-right: 1px solid var(--line);
    background: #fbfcfc;
    display: flex;
    flex-direction: column;
    gap: 22px;
  }
  .brand-mark {
    display: flex;
    align-items: center;
    gap: 11px;
    min-height: 42px;
  }
  .mark {
    width: 36px;
    height: 36px;
    border-radius: 8px;
    background: var(--ink);
    color: white;
    display: grid;
    place-items: center;
    font: 700 16px/1 var(--mono);
  }
  .brand-text strong { display: block; font-size: 15px; }
  .brand-text span { display: block; color: var(--muted); font-size: 12px; }
  .lang-switch {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 6px;
  }
  .lang-switch button[aria-pressed="true"] {
    background: var(--accent);
    border-color: var(--accent);
    color: #fff;
  }
  .rail-group { display: grid; gap: 8px; }
  .rail-label {
    color: var(--muted);
    font-size: 11px;
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }
  .rail-link {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 8px;
    padding: 8px 0;
    border-bottom: 1px solid var(--line);
    color: var(--ink);
    text-decoration: none;
  }
  .rail-link code { color: var(--muted); font-family: var(--mono); font-size: 12px; }
  .rail-note {
    margin-top: auto;
    padding-top: 14px;
    border-top: 1px solid var(--line);
    color: var(--muted);
    font-size: 12px;
  }
  .main-plane {
    min-width: 0;
    padding: 22px 26px 28px;
  }
  .topbar {
    display: flex;
    justify-content: space-between;
    gap: 18px;
    align-items: flex-start;
    padding-bottom: 20px;
    border-bottom: 1px solid var(--line);
  }
  h1 {
    margin: 0;
    font-size: 30px;
    line-height: 1.05;
    letter-spacing: 0;
  }
  .topbar p {
    margin: 7px 0 0;
    color: var(--muted);
    max-width: 760px;
  }
  .topbar-meta {
    display: flex;
    flex-wrap: wrap;
    justify-content: flex-end;
    gap: 8px;
    min-width: 250px;
  }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    min-height: 30px;
    padding: 4px 9px;
    border: 1px solid var(--line);
    border-radius: 999px;
    background: var(--surface);
    color: var(--muted);
    font-size: 12px;
    white-space: nowrap;
  }
  .chip strong { color: var(--ink); font-weight: 600; }
  .command-control {
    margin-top: 18px;
    background: var(--surface);
    border: 1px solid var(--line);
    border-radius: 8px;
    box-shadow: var(--shadow);
    padding: 18px;
  }
  .onboarding-deck {
    margin-top: 18px;
    display: grid;
    grid-template-columns: minmax(280px, 1.15fr) minmax(260px, 0.85fr);
    gap: 16px;
    align-items: stretch;
  }
  .carrier-map {
    min-height: 238px;
    border: 1px solid var(--line);
    border-radius: 8px;
    background:
      radial-gradient(circle at 50% 36%, rgba(29, 111, 121, .22), transparent 34%),
      linear-gradient(180deg, #ffffff 0%, #eef5f5 100%);
    padding: 18px;
    position: relative;
    overflow: hidden;
  }
  .carrier-map::before {
    content: "";
    position: absolute;
    inset: 16px;
    background:
      linear-gradient(90deg, transparent 49%, rgba(29,111,121,.16) 50%, transparent 51%),
      linear-gradient(0deg, transparent 49%, rgba(29,111,121,.10) 50%, transparent 51%);
    background-size: 42px 42px;
    opacity: .75;
  }
  .carrier-map::after {
    content: "";
    position: absolute;
    left: 50%;
    top: 28px;
    bottom: 28px;
    width: 1px;
    background: linear-gradient(180deg, transparent, rgba(29,111,121,.34), transparent);
  }
  .carrier-node {
    position: relative;
    z-index: 1;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 108px;
    min-height: 42px;
    margin: 8px;
    padding: 8px 11px;
    border: 1px solid var(--line-strong);
    border-radius: 8px;
    background: rgba(255,255,255,.88);
    color: var(--ink);
    font-weight: 650;
  }
  .carrier-core-node {
    position: relative;
    z-index: 1;
    display: grid;
    place-items: center;
    min-width: 172px;
    min-height: 62px;
    margin: 8px 14px;
    padding: 10px 15px;
    border: 1px solid rgba(23, 27, 31, .92);
    border-radius: 8px;
    background: #171b1f;
    color: #fff;
    box-shadow: 0 12px 30px rgba(23, 27, 31, .18);
    font-weight: 760;
  }
  .carrier-core-node small {
    display: block;
    margin-top: 3px;
    color: rgba(255,255,255,.68);
    font: 500 11px/1.2 var(--mono);
  }
  .carrier-node.core {
    min-width: 150px;
    min-height: 56px;
    background: var(--ink);
    color: #fff;
  }
  .carrier-row {
    position: relative;
    display: flex;
    justify-content: center;
    flex-wrap: wrap;
  }
  .carrier-metrics {
    position: relative;
    z-index: 1;
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: 8px;
    margin-top: 14px;
  }
  .carrier-metric {
    border-top: 1px solid var(--line);
    padding-top: 8px;
    color: var(--muted);
    font-size: 12px;
  }
  .carrier-metric strong {
    display: block;
    color: var(--ink);
    font-size: 13px;
  }
  .tutorial-steps {
    border: 1px solid var(--line);
    border-radius: 8px;
    background: var(--surface);
    padding: 17px;
  }
  .tutorial-step {
    display: grid;
    grid-template-columns: 30px 1fr;
    gap: 10px;
    padding: 11px 0;
    border-bottom: 1px solid var(--line);
  }
  .tutorial-step:last-child { border-bottom: 0; }
  .step-index {
    width: 28px;
    height: 28px;
    border-radius: 50%;
    background: var(--accent-soft);
    color: var(--accent);
    display: grid;
    place-items: center;
    font-weight: 700;
  }
  .tutorial-step strong { display: block; margin-bottom: 2px; }
  .tutorial-step code { font-family: var(--mono); color: var(--accent); }
  .panel-head {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    gap: 14px;
    margin-bottom: 12px;
  }
  .panel-title { margin: 0; font-size: 15px; line-height: 1.2; }
  .panel-kicker {
    color: var(--muted);
    font-size: 12px;
    margin-top: 3px;
  }
  .run-controls {
    display: grid;
    gap: 10px;
  }
  .run-control-row {
    display: grid;
    grid-template-columns: minmax(240px, 1fr) minmax(220px, 320px) auto;
    gap: 10px;
    align-items: start;
  }
  .runtime-grid {
    margin-top: 18px;
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
    gap: 16px;
    align-items: start;
  }
  .runtime-grid .wide { grid-column: 1 / -1; }
  .panel {
    min-width: 0;
    background: var(--surface);
    border: 1px solid var(--line);
    border-radius: 8px;
    overflow: hidden;
  }
  .panel .panel-head {
    padding: 15px 16px 0;
  }
  .status-bar {
    margin: 0 16px 12px;
    min-height: 20px;
    color: var(--muted);
    font-size: 12px;
  }
  .status-bar.error { color: var(--bad); }
  .table-wrap {
    overflow: auto;
    max-height: 360px;
    border-top: 1px solid var(--line);
  }
  table {
    width: 100%;
    border-collapse: collapse;
    font-size: 12px;
  }
  th {
    position: sticky;
    top: 0;
    z-index: 1;
    background: #fbfcfc;
    color: var(--muted);
    text-align: left;
    padding: 9px 12px;
    border-bottom: 1px solid var(--line);
    font-weight: 600;
  }
  td {
    padding: 9px 12px;
    border-bottom: 1px solid #edf1f2;
    vertical-align: top;
    word-break: break-word;
  }
  tr:hover td { background: #f8fbfb; }
  .empty { color: var(--muted); font-style: italic; }
  .pill {
    display: inline-flex;
    align-items: center;
    min-height: 22px;
    padding: 2px 7px;
    border: 1px solid currentColor;
    border-radius: 999px;
    font-size: 11px;
    white-space: nowrap;
  }
  .sig-ok, .state-awake { color: var(--ok); }
  .sig-fail { color: var(--bad); }
  .state-sleeping, .state-migrating { color: var(--warn); }
  .state-created { color: var(--accent); }
  .stream-toolbar {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin: 0 16px 12px;
  }
  .sse-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--line-strong);
    display: inline-block;
  }
  .sse-dot.live {
    background: var(--ok);
    box-shadow: 0 0 0 5px rgba(15, 122, 79, 0.12);
  }
  .webhook-form {
    padding: 0 16px 14px;
  }
  .webhook-form .run-control-row {
    grid-template-columns: minmax(170px, 260px) minmax(260px, 1fr) auto;
  }
  footer {
    color: var(--muted);
    font-size: 12px;
    margin-top: 18px;
    padding-top: 14px;
    border-top: 1px solid var(--line);
  }
  @media (max-width: 980px) {
    .workspace-shell { grid-template-columns: 1fr; }
    .rail { position: static; height: auto; border-right: 0; border-bottom: 1px solid var(--line); }
    .rail-note { margin-top: 0; }
    .topbar { flex-direction: column; }
    .topbar-meta { justify-content: flex-start; }
    .runtime-grid { grid-template-columns: 1fr; }
    .onboarding-deck { grid-template-columns: 1fr; }
    .run-control-row, .webhook-form .run-control-row { grid-template-columns: 1fr; }
  }
</style>
</head>
"#;

const WEBUI_BODY: &str = r##"<body>
<div class="workspace-shell">
  <aside class="rail">
    <div class="brand-mark">
      <div class="mark">Z</div>
      <div class="brand-text">
        <strong>Zaion</strong>
        <span data-i18n="rail.subtitle">本地母舰控制台</span>
      </div>
    </div>

    <div class="lang-switch" aria-label="Language">
      <button type="button" id="lang-zh" data-lang-button="zh">中文</button>
      <button type="button" id="lang-en" data-lang-button="en">EN</button>
    </div>

    <nav class="rail-group" aria-label="Command map">
      <div class="rail-label" data-i18n="rail.launcher">入口地图</div>
      <a class="rail-link" href="#tutorial"><span data-i18n="rail.tutorial">新手航线</span><code>tutorial</code></a>
      <a class="rail-link" href="#command-control"><span data-i18n="rail.webui">浏览器 WebUI</span><code>dashboard</code></a>
      <a class="rail-link" href="#sec-runs"><span data-i18n="rail.runs">签名任务</span><code>/v1/runs</code></a>
      <a class="rail-link" href="#sec-events"><span data-i18n="rail.streams">实时流</span><code>events/ws</code></a>
      <a class="rail-link" href="#sec-webhooks"><span data-i18n="rail.hooks">网关钩子</span><code>webhooks</code></a>
    </nav>

    <div class="rail-group">
      <div class="rail-label" data-i18n="rail.relation">命令关系</div>
      <div class="rail-link"><span data-i18n="rail.terminal">神经拓扑 TUI</span><code>zaion</code></div>
      <div class="rail-link"><span data-i18n="rail.runtime">完整运行体</span><code>zaion start</code></div>
      <div class="rail-link"><span data-i18n="rail.gateway">HTTP 服务</span><code>gateway start</code></div>
    </div>

    <div class="rail-note" data-i18n="rail.note">
      默认读取全局 Zaion home；项目级覆盖保持显式。
    </div>
  </aside>

  <main class="main-plane">
    <header class="topbar">
      <div>
        <h1 data-i18n="hero.title">Zaion 母舰控制台</h1>
        <p data-i18n="hero.subtitle">一个简单可上手的星空母舰入口：先完成身份和模型，再启动运行体，最后用 Telegram 或 WebUI 做基测。</p>
      </div>
      <div class="topbar-meta">
        <span class="chip"><span data-i18n="chip.gateway">网关</span> <strong id="gateway-state">local</strong></span>
        <span class="chip"><span data-i18n="chip.base">地址</span> <strong id="base-url"></strong></span>
        <span class="chip"><span data-i18n="chip.clock">时间</span> <strong id="clock">--:--:--</strong></span>
      </div>
    </header>

    <section class="onboarding-deck" id="tutorial">
      <div class="carrier-map" aria-label="Zaion neural topology">
        <div class="carrier-row">
          <span class="carrier-node" data-i18n="topology.identity">身份</span>
          <span class="carrier-core-node"><span data-i18n="topology.core">Zaion 神经母舰</span><small>launch graph</small></span>
          <span class="carrier-node" data-i18n="topology.proof">证明链</span>
        </div>
        <div class="carrier-row">
          <span class="carrier-node" data-i18n="topology.runtime">运行体</span>
          <span class="carrier-node" data-i18n="topology.tg">Telegram</span>
          <span class="carrier-node" data-i18n="topology.webui">WebUI</span>
        </div>
        <div class="carrier-row">
          <span class="carrier-node" data-i18n="topology.memory">记忆</span>
          <span class="carrier-node" data-i18n="topology.aci">ACI AST</span>
          <span class="carrier-node" data-i18n="topology.ouroboros">Ouroboros 自愈</span>
        </div>
        <div class="carrier-metrics" aria-label="Launch readiness">
          <div class="carrier-metric"><strong data-i18n="metric.identity">身份</strong><span data-i18n="metric.identityBody">Ed25519 / principal</span></div>
          <div class="carrier-metric"><strong data-i18n="metric.runtime">运行体</strong><span data-i18n="metric.runtimeBody">start / gateway</span></div>
          <div class="carrier-metric"><strong data-i18n="metric.channel">通道</strong><span data-i18n="metric.channelBody">Telegram baseline</span></div>
        </div>
      </div>
      <div class="tutorial-steps">
        <div class="panel-head">
          <div>
            <h2 class="panel-title" data-i18n="tutorial.title">三步启动 Zaion</h2>
            <div class="panel-kicker" data-i18n="tutorial.kicker">不用先理解全部系统，照这条航线走就能完成第一次对话。</div>
          </div>
        </div>
        <div class="tutorial-step">
          <span class="step-index">1</span>
          <div><strong data-i18n="tutorial.step1.title">配置身份和模型</strong><span data-i18n="tutorial.step1.body">运行 </span><code>zaion onboard</code></div>
        </div>
        <div class="tutorial-step">
          <span class="step-index">2</span>
          <div><strong data-i18n="tutorial.step2.title">启动完整运行体</strong><span data-i18n="tutorial.step2.body">运行 </span><code>zaion start</code></div>
        </div>
        <div class="tutorial-step">
          <span class="step-index">3</span>
          <div><strong data-i18n="tutorial.step3.title">做 Telegram 基测</strong><span data-i18n="tutorial.step3.body">先运行 </span><code>zaion tg doctor</code><span data-i18n="tutorial.step3.tail">，再给机器人发 /start 或一句话。</span></div>
        </div>
      </div>
    </section>

    <section class="command-control" id="command-control">
      <div class="panel-head">
        <div>
          <h2 class="panel-title" data-i18n="control.title">指挥控制</h2>
          <div class="panel-kicker" data-i18n="control.kicker">通过网关提交一条签名 ACP 任务。</div>
        </div>
        <span class="chip" data-i18n="control.retry">幂等重试</span>
      </div>
      <form class="run-controls" id="run-submit-form">
        <div class="run-control-row">
          <input id="run-task-input" name="task" autocomplete="off" placeholder="任务 / Task" />
          <input id="run-principal-input" name="submitter_principal" autocomplete="off" placeholder="提交身份 / Submitter principal" />
          <button class="primary" type="submit" data-i18n="control.submit">提交任务</button>
        </div>
        <input id="run-idempotency-key-input" name="idempotency_key" type="hidden" />
      </form>
      <div class="status-bar" id="runs-status" data-i18n="status.loadingRuns">正在加载任务...</div>
    </section>

    <div class="runtime-grid">
      <section class="panel" id="sec-procs">
        <div class="panel-head">
          <div>
            <h2 class="panel-title" data-i18n="process.title">进程</h2>
            <div class="panel-kicker" data-i18n="process.kicker">身份、状态、工作区与密钥存在性。</div>
          </div>
          <span class="chip"><strong id="process-count">0</strong> <span data-i18n="process.active">活跃</span></span>
        </div>
        <div class="status-bar" id="procs-status" data-i18n="status.loadingProcesses">正在加载进程...</div>
        <div class="table-wrap">
          <table>
            <thead>
              <tr>
                <th data-i18n="table.principal">身份</th>
                <th data-i18n="table.state">状态</th>
                <th data-i18n="table.workspace">工作区</th>
                <th data-i18n="table.key">密钥</th>
              </tr>
            </thead>
            <tbody id="procs-body">
              <tr><td colspan="4" class="empty" data-i18n="status.fetching">正在获取...</td></tr>
            </tbody>
          </table>
        </div>
      </section>

      <section class="panel" id="sec-runs">
        <div class="panel-head">
          <div>
            <h2 class="panel-title" data-i18n="runs.title">ACP 任务</h2>
            <div class="panel-kicker" data-i18n="runs.kicker">检查流或取消活跃工作。</div>
          </div>
          <span class="chip"><strong id="run-count">0</strong> <span data-i18n="runs.recent">最近</span></span>
        </div>
        <div class="table-wrap">
          <table>
            <thead>
              <tr>
                <th data-i18n="table.run">任务</th>
                <th data-i18n="table.status">状态</th>
                <th data-i18n="table.task">内容</th>
                <th data-i18n="table.sig">签名</th>
                <th data-i18n="table.stream">流</th>
                <th data-i18n="table.action">动作</th>
              </tr>
            </thead>
            <tbody id="runs-body">
              <tr><td colspan="6" class="empty" data-i18n="status.fetching">正在获取...</td></tr>
            </tbody>
          </table>
        </div>
      </section>

      <section class="panel" id="sec-webhooks">
        <div class="panel-head">
          <div>
            <h2 class="panel-title" data-i18n="webhooks.title">Webhooks</h2>
            <div class="panel-kicker" data-i18n="webhooks.kicker">重载订阅并分发测试载荷。</div>
          </div>
          <span class="chip"><strong id="webhook-count">0</strong> <span data-i18n="webhooks.subs">订阅</span></span>
        </div>
        <form class="run-controls webhook-form" id="webhook-dispatch-form">
          <div class="run-control-row">
            <input id="webhook-event-input" name="event" autocomplete="off" placeholder="事件名 / Event name" />
            <textarea id="webhook-payload-input" name="payload" placeholder='{"source":"webui"}'></textarea>
            <button class="primary" type="submit" data-i18n="webhooks.dispatch">分发</button>
          </div>
          <button type="button" id="webhook-reload-button" data-i18n="webhooks.reload">重载订阅</button>
        </form>
        <div class="status-bar" id="webhooks-status" data-i18n="status.loadingWebhooks">正在加载 webhooks...</div>
        <div class="table-wrap">
          <table>
            <thead>
              <tr>
                <th data-i18n="table.name">名称</th>
                <th data-i18n="table.events">事件</th>
                <th data-i18n="table.status">状态</th>
                <th data-i18n="table.secret">密钥</th>
              </tr>
            </thead>
            <tbody id="webhooks-body">
              <tr><td colspan="4" class="empty" data-i18n="status.fetching">正在获取...</td></tr>
            </tbody>
          </table>
        </div>
      </section>

      <section class="panel" id="sec-events">
        <div class="panel-head">
          <div>
            <h2 class="panel-title"><span class="sse-dot" id="sse-dot"></span> <span data-i18n="events.title">账本流</span></h2>
            <div class="panel-kicker" data-i18n="events.kicker">全局事件、选中任务流、operation SSE 与 operation WebSocket。</div>
          </div>
          <span class="chip"><strong id="event-count">0</strong> <span data-i18n="events.visible">可见</span></span>
        </div>
        <div class="stream-toolbar">
          <button type="button" id="operation-live-button" data-i18n="events.poll">轮询 operations</button>
          <button type="button" id="operation-ws-button" data-i18n="events.connectWs">连接 WebSocket</button>
          <button type="button" id="operation-ws-disconnect-button" data-i18n="events.disconnectWs">断开 WS</button>
          <button type="button" id="operation-cursor-reset-button" data-i18n="events.resetCursor">重置游标</button>
        </div>
        <div class="status-bar" id="operations-status" data-i18n="status.operationsIdle">direct operation stream 空闲</div>
        <div class="status-bar" id="operations-ws-status" data-i18n="status.wsIdle">operation WebSocket 空闲</div>
        <div class="status-bar" id="events-status" data-i18n="status.connecting">正在连接...</div>
        <div class="table-wrap">
          <table>
            <thead>
              <tr>
                <th data-i18n="table.time">时间</th>
                <th data-i18n="table.principal">身份</th>
                <th data-i18n="table.type">类型</th>
                <th data-i18n="table.sig">签名</th>
              </tr>
            </thead>
            <tbody id="events-body">
              <tr><td colspan="4" class="empty" data-i18n="status.waitingEvents">等待事件...</td></tr>
            </tbody>
          </table>
        </div>
      </section>
    </div>

    <footer data-i18n="footer">
      Zaion 浏览器 WebUI。自动刷新：3 秒。CLI 兼容视图仍保留 `zaion dashboard status` 与 `zaion dashboard trace`。
    </footer>
  </main>
</div>
"##;

const WEBUI_SCRIPT: &str = r#"<script>
(function() {
  'use strict';

  const BASE = window.location.origin;
  document.getElementById('base-url').textContent = BASE.replace(/^https?:\/\//, '');

  const I18N = {
    zh: {
      'rail.subtitle': '本地母舰控制台',
      'rail.launcher': '入口地图',
      'rail.tutorial': '新手航线',
      'rail.webui': '浏览器 WebUI',
      'rail.runs': '签名任务',
      'rail.streams': '实时流',
      'rail.hooks': '网关钩子',
      'rail.relation': '命令关系',
      'rail.terminal': '神经拓扑 TUI',
      'rail.runtime': '完整运行体',
      'rail.gateway': 'HTTP 服务',
      'rail.note': '默认读取全局 Zaion home；项目级覆盖保持显式。',
      'hero.title': 'Zaion 母舰控制台',
      'hero.subtitle': '一个简单可上手的星空母舰入口：先完成身份和模型，再启动运行体，最后用 Telegram 或 WebUI 做基测。',
      'chip.gateway': '网关',
      'chip.base': '地址',
      'chip.clock': '时间',
      'topology.identity': '身份',
      'topology.core': 'Zaion 神经母舰',
      'topology.proof': '证明链',
      'topology.runtime': '运行体',
      'topology.tg': 'Telegram',
      'topology.webui': 'WebUI',
      'topology.memory': '记忆',
      'topology.aci': 'ACI AST',
      'topology.ouroboros': 'Ouroboros 自愈',
      'metric.identity': '身份',
      'metric.identityBody': 'Ed25519 / principal',
      'metric.runtime': '运行体',
      'metric.runtimeBody': 'start / gateway',
      'metric.channel': '通道',
      'metric.channelBody': 'Telegram baseline',
      'tutorial.title': '三步启动 Zaion',
      'tutorial.kicker': '不用先理解全部系统，照这条航线走就能完成第一次对话。',
      'tutorial.step1.title': '配置身份和模型',
      'tutorial.step1.body': '运行 ',
      'tutorial.step2.title': '启动完整运行体',
      'tutorial.step2.body': '运行 ',
      'tutorial.step3.title': '做 Telegram 基测',
      'tutorial.step3.body': '先运行 ',
      'tutorial.step3.tail': '，再给机器人发 /start 或一句话。',
      'control.title': '指挥控制',
      'control.kicker': '通过网关提交一条签名 ACP 任务。',
      'control.retry': '幂等重试',
      'control.submit': '提交任务',
      'process.title': '进程',
      'process.kicker': '身份、状态、工作区与密钥存在性。',
      'process.active': '活跃',
      'runs.title': 'ACP 任务',
      'runs.kicker': '检查流或取消活跃工作。',
      'runs.recent': '最近',
      'webhooks.title': 'Webhooks',
      'webhooks.kicker': '重载订阅并分发测试载荷。',
      'webhooks.subs': '订阅',
      'webhooks.dispatch': '分发',
      'webhooks.reload': '重载订阅',
      'events.title': '账本流',
      'events.kicker': '全局事件、选中任务流、operation SSE 与 operation WebSocket。',
      'events.visible': '可见',
      'events.poll': '轮询 operations',
      'events.connectWs': '连接 WebSocket',
      'events.disconnectWs': '断开 WS',
      'events.resetCursor': '重置游标',
      'table.principal': '身份',
      'table.state': '状态',
      'table.workspace': '工作区',
      'table.key': '密钥',
      'table.run': '任务',
      'table.status': '状态',
      'table.task': '内容',
      'table.sig': '签名',
      'table.stream': '流',
      'table.action': '动作',
      'table.name': '名称',
      'table.events': '事件',
      'table.secret': '密钥',
      'table.time': '时间',
      'table.type': '类型',
      'status.loadingRuns': '正在加载任务...',
      'status.loadingProcesses': '正在加载进程...',
      'status.loadingWebhooks': '正在加载 webhooks...',
      'status.fetching': '正在获取...',
      'status.operationsIdle': 'direct operation stream 空闲',
      'status.wsIdle': 'operation WebSocket 空闲',
      'status.connecting': '正在连接...',
      'status.waitingEvents': '等待事件...',
      'footer': 'Zaion 浏览器 WebUI。自动刷新：3 秒。CLI 兼容视图仍保留 `zaion dashboard status` 与 `zaion dashboard trace`。'
    },
    en: {
      'rail.subtitle': 'local carrier console',
      'rail.launcher': 'Launcher map',
      'rail.tutorial': 'Beginner route',
      'rail.webui': 'Browser WebUI',
      'rail.runs': 'Signed runs',
      'rail.streams': 'Live streams',
      'rail.hooks': 'Gateway hooks',
      'rail.relation': 'Command relation',
      'rail.terminal': 'Neural topology TUI',
      'rail.runtime': 'Full runtime',
      'rail.gateway': 'HTTP service',
      'rail.note': 'Global Zaion home is the default; project overrides stay explicit.',
      'hero.title': 'Zaion Carrier Console',
      'hero.subtitle': 'A simple starship-carrier entry: configure identity and model, start the runtime, then test Telegram or WebUI.',
      'chip.gateway': 'Gateway',
      'chip.base': 'Base',
      'chip.clock': 'Clock',
      'topology.identity': 'Identity',
      'topology.core': 'Zaion Neural Carrier',
      'topology.proof': 'Proof chain',
      'topology.runtime': 'Runtime',
      'topology.tg': 'Telegram',
      'topology.webui': 'WebUI',
      'topology.memory': 'Memory',
      'topology.aci': 'ACI AST',
      'topology.ouroboros': 'Ouroboros heal',
      'metric.identity': 'Identity',
      'metric.identityBody': 'Ed25519 / principal',
      'metric.runtime': 'Runtime',
      'metric.runtimeBody': 'start / gateway',
      'metric.channel': 'Channel',
      'metric.channelBody': 'Telegram baseline',
      'tutorial.title': 'Start Zaion in 3 steps',
      'tutorial.kicker': 'You do not need the whole system first; follow this route for your first conversation.',
      'tutorial.step1.title': 'Configure identity and model',
      'tutorial.step1.body': 'Run ',
      'tutorial.step2.title': 'Start the full runtime',
      'tutorial.step2.body': 'Run ',
      'tutorial.step3.title': 'Run a Telegram baseline test',
      'tutorial.step3.body': 'Run ',
      'tutorial.step3.tail': ', then message /start or a sentence to the bot.',
      'control.title': 'Command control',
      'control.kicker': 'Submit a signed ACP run through the gateway.',
      'control.retry': 'idempotent retry',
      'control.submit': 'Submit run',
      'process.title': 'Processes',
      'process.kicker': 'Principal, state, workspace, and key presence.',
      'process.active': 'active',
      'runs.title': 'ACP runs',
      'runs.kicker': 'Inspect streams or cancel active work.',
      'runs.recent': 'recent',
      'webhooks.title': 'Webhooks',
      'webhooks.kicker': 'Reload subscriptions and dispatch test payloads.',
      'webhooks.subs': 'subscriptions',
      'webhooks.dispatch': 'Dispatch',
      'webhooks.reload': 'Reload subscriptions',
      'events.title': 'Ledger stream',
      'events.kicker': 'Global events, selected run stream, operation SSE, and operation WebSocket.',
      'events.visible': 'visible',
      'events.poll': 'Poll operations',
      'events.connectWs': 'Connect WebSocket',
      'events.disconnectWs': 'Disconnect WS',
      'events.resetCursor': 'Reset cursor',
      'table.principal': 'Principal',
      'table.state': 'State',
      'table.workspace': 'Workspace',
      'table.key': 'Key',
      'table.run': 'Run',
      'table.status': 'Status',
      'table.task': 'Task',
      'table.sig': 'Sig',
      'table.stream': 'Stream',
      'table.action': 'Action',
      'table.name': 'Name',
      'table.events': 'Events',
      'table.secret': 'Secret',
      'table.time': 'Time',
      'table.type': 'Type',
      'status.loadingRuns': 'loading runs...',
      'status.loadingProcesses': 'loading processes...',
      'status.loadingWebhooks': 'loading webhooks...',
      'status.fetching': 'fetching...',
      'status.operationsIdle': 'direct operation stream idle',
      'status.wsIdle': 'operation WebSocket idle',
      'status.connecting': 'connecting...',
      'status.waitingEvents': 'waiting for events...',
      'footer': 'Zaion browser WebUI. Auto-refresh: 3s. CLI compatibility views remain `zaion dashboard status` and `zaion dashboard trace`.'
    }
  };

  let currentLang = localStorage.getItem('zaion.webui.lang') || 'zh';

  function applyLanguage(lang) {
    currentLang = lang === 'en' ? 'en' : 'zh';
    document.documentElement.lang = currentLang === 'zh' ? 'zh-CN' : 'en';
    localStorage.setItem('zaion.webui.lang', currentLang);
    document.querySelectorAll('[data-i18n]').forEach((el) => {
      const key = el.getAttribute('data-i18n');
      const value = I18N[currentLang][key];
      if (value !== undefined) el.textContent = value;
    });
    document.querySelectorAll('[data-lang-button]').forEach((button) => {
      button.setAttribute('aria-pressed', button.getAttribute('data-lang-button') === currentLang ? 'true' : 'false');
    });
  }

  document.querySelectorAll('[data-lang-button]').forEach((button) => {
    button.addEventListener('click', () => applyLanguage(button.getAttribute('data-lang-button')));
  });
  applyLanguage(currentLang);

  function tick() {
    const now = new Date();
    document.getElementById('clock').textContent = now.toTimeString().slice(0, 8);
  }
  tick();
  setInterval(tick, 1000);

  function text(id, value) {
    const el = document.getElementById(id);
    if (el) el.textContent = value;
  }

  function sigBadge(ok) {
    if (ok === null || ok === undefined) return '<span class="sig-fail pill">unknown</span>';
    return ok
      ? '<span class="sig-ok pill">ok</span>'
      : '<span class="sig-fail pill">fail</span>';
  }

  function stateClass(s) {
    if (!s) return '';
    const low = s.toLowerCase();
    if (low.includes('awake')) return 'state-awake';
    if (low.includes('sleep')) return 'state-sleeping';
    if (low.includes('migrat')) return 'state-migrating';
    return 'state-created';
  }

  function short(str, n) {
    if (!str) return '-';
    return str.length > n ? str.slice(0, n) + '...' : str;
  }

  function escapeHtml(value) {
    return String(value || '')
      .replace(/&/g, '&amp;')
      .replace(/</g, '&lt;')
      .replace(/>/g, '&gt;')
      .replace(/"/g, '&quot;')
      .replace(/'/g, '&#39;');
  }

  function timeAgo(iso) {
    if (!iso) return '-';
    const diff = Math.max(0, Math.floor((Date.now() - new Date(iso).getTime()) / 1000));
    if (diff < 60) return diff + 's ago';
    if (diff < 3600) return Math.floor(diff / 60) + 'm ago';
    return Math.floor(diff / 3600) + 'h ago';
  }

  function setStatus(id, msg, isErr) {
    const el = document.getElementById(id);
    if (!el) return;
    el.textContent = msg;
    el.className = 'status-bar' + (isErr ? ' error' : '');
  }

  function canCancelRun(status) {
    const low = (status || '').toLowerCase();
    return ['queued', 'pending', 'created', 'running', 'in_progress'].includes(low);
  }

  let selectedPrincipalId = '';

  function fetchProcesses() {
    fetch(BASE + '/api/v1/processes')
      .then(r => r.json())
      .then(data => {
        const procs = data.processes || [];
        text('process-count', String(procs.length));
        text('gateway-state', 'healthy');
        setStatus('procs-status', `${procs.length} process(es), updated ${new Date().toLocaleTimeString()}`);
        const principalInput = document.getElementById('run-principal-input');
        const firstPrincipal = procs.find(p => p.principal_id && p.principal_id !== 'anonymous');
        if (firstPrincipal) {
          selectedPrincipalId = firstPrincipal.principal_id;
          if (principalInput && !principalInput.value) principalInput.value = selectedPrincipalId;
        }
        const tbody = document.getElementById('procs-body');
        if (procs.length === 0) {
          tbody.innerHTML = '<tr><td colspan="4" class="empty">no processes found</td></tr>';
          return;
        }
        tbody.innerHTML = procs.map(p => {
          const sc = stateClass(p.state);
          const hasSig = (p.public_key_hex && p.public_key_hex.length > 0) ? true : null;
          const principal = escapeHtml(p.principal_id || '');
          const workspace = escapeHtml(p.workspace || p.workspace_id || '');
          return `<tr>
            <td title="${principal}"><code>${escapeHtml(short(p.principal_id, 24))}</code></td>
            <td><span class="${sc} pill">${escapeHtml(p.state || '?')}</span></td>
            <td>${escapeHtml(short(workspace, 26))}</td>
            <td>${sigBadge(hasSig)}</td>
          </tr>`;
        }).join('');
      })
      .catch(e => {
        text('gateway-state', 'offline');
        setStatus('procs-status', 'error: ' + e.message, true);
      });
  }

  let runIdempotencyFingerprint = '';

  function runIdempotencyKey(task, submitter) {
    const input = document.getElementById('run-idempotency-key-input');
    const fingerprint = submitter + '\n' + task;
    if (input && input.value && runIdempotencyFingerprint === fingerprint) {
      return input.value;
    }
    const random = (window.crypto && window.crypto.randomUUID)
      ? window.crypto.randomUUID()
      : Math.random().toString(36).slice(2);
    const key = 'webui-' + Date.now().toString(36) + '-' + random;
    runIdempotencyFingerprint = fingerprint;
    if (input) input.value = key;
    return key;
  }

  function submitRun(event) {
    if (event) event.preventDefault();
    const taskInput = document.getElementById('run-task-input');
    const principalInput = document.getElementById('run-principal-input');
    const task = (taskInput && taskInput.value || '').trim();
    const submitter = (principalInput && principalInput.value || selectedPrincipalId || '').trim();
    if (!task || !submitter) {
      setStatus('runs-status', 'task and submitter principal are required', true);
      return;
    }
    const idempotencyKey = runIdempotencyKey(task, submitter);
    setStatus('runs-status', 'submitting signed ACP run...');
    fetch(BASE + '/v1/runs', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json', 'Idempotency-Key': idempotencyKey },
      body: JSON.stringify({ task, submitter_principal: submitter, idempotency_key: idempotencyKey })
    })
      .then(async r => {
        const data = await r.json().catch(() => ({}));
        if (!r.ok) throw new Error(data.error || r.statusText);
        if (taskInput) taskInput.value = '';
        const keyInput = document.getElementById('run-idempotency-key-input');
        if (keyInput) keyInput.value = '';
        runIdempotencyFingerprint = '';
        const verb = data.idempotency_reused ? 'reused' : 'submitted';
        setStatus('runs-status', `${verb} ${data.run_id || 'run'}`);
        fetchRuns();
      })
      .catch(e => {
        setStatus('runs-status', 'submit error: ' + e.message, true);
      });
  }

  function cancelRun(runId) {
    if (!runId) return;
    setStatus('runs-status', `cancelling ${short(runId, 18)}...`);
    fetch(BASE + '/v1/runs/' + encodeURIComponent(runId), { method: 'DELETE' })
      .then(async r => {
        const data = await r.json().catch(() => ({}));
        if (!r.ok) throw new Error(data.error || r.statusText);
        setStatus('runs-status', `cancelled ${data.cancelled || runId}`);
        fetchRuns();
      })
      .catch(e => {
        setStatus('runs-status', 'cancel error: ' + e.message, true);
      });
  }

  function fetchRuns() {
    fetch(BASE + '/v1/runs')
      .then(r => r.json())
      .then(data => {
        const runs = data.runs || [];
        text('run-count', String(runs.length));
        setStatus('runs-status', `${runs.length} run(s), updated ${new Date().toLocaleTimeString()}`);
        const tbody = document.getElementById('runs-body');
        if (runs.length === 0) {
          tbody.innerHTML = '<tr><td colspan="6" class="empty">no runs found</td></tr>';
          return;
        }
        tbody.innerHTML = runs.map(r => {
          const statusOk = ['completed', 'done', 'success'].includes((r.status || '').toLowerCase());
          const statusFail = ['failed', 'error', 'cancelled'].includes((r.status || '').toLowerCase());
          const statusClass = statusOk ? 'state-awake' : statusFail ? 'sig-fail' : 'state-sleeping';
          const hasSig = r.submitter_principal ? true : null;
          const runId = escapeHtml(r.run_id || '');
          const streamAction = selectedRunId === r.run_id
            ? '<span class="sig-ok pill">live</span>'
            : `<button type="button" data-inspect-run="${runId}">Inspect</button>`;
          const action = canCancelRun(r.status)
            ? `<button class="danger" type="button" data-cancel-run="${runId}">Cancel</button>`
            : '-';
          return `<tr>
            <td title="${runId}"><code>${escapeHtml(short(r.run_id, 18))}</code></td>
            <td><span class="${statusClass} pill">${escapeHtml(r.status || '?')}</span></td>
            <td title="${escapeHtml(r.task || '')}">${escapeHtml(short(r.task, 34))}</td>
            <td>${sigBadge(hasSig)}</td>
            <td>${streamAction}</td>
            <td>${action}</td>
          </tr>`;
        }).join('');
        tbody.querySelectorAll('button[data-inspect-run]').forEach(button => {
          button.addEventListener('click', () => inspectRunStream(button.getAttribute('data-inspect-run')));
        });
        tbody.querySelectorAll('button[data-cancel-run]').forEach(button => {
          button.addEventListener('click', () => cancelRun(button.getAttribute('data-cancel-run')));
        });
      })
      .catch(e => {
        setStatus('runs-status', 'error: ' + e.message, true);
      });
  }

  function fetchWebhooks() {
    fetch(BASE + '/api/v1/webhooks')
      .then(r => r.json())
      .then(data => {
        renderWebhooks(data.subscriptions || []);
      })
      .catch(e => {
        setStatus('webhooks-status', 'error: ' + e.message, true);
      });
  }

  function renderWebhooks(subscriptions) {
    text('webhook-count', String(subscriptions.length));
    setStatus('webhooks-status', `${subscriptions.length} subscription(s), updated ${new Date().toLocaleTimeString()}`);
    const tbody = document.getElementById('webhooks-body');
    if (!subscriptions.length) {
      tbody.innerHTML = '<tr><td colspan="4" class="empty">no webhook subscriptions found</td></tr>';
      return;
    }
    tbody.innerHTML = subscriptions.map(s => {
      const events = Array.isArray(s.events) ? s.events.join(', ') : '';
      return `<tr>
        <td title="${escapeHtml(s.url || '')}">${escapeHtml(short(s.name, 24))}</td>
        <td>${escapeHtml(short(events, 34))}</td>
        <td><span class="pill">${escapeHtml(s.status || '?')}</span></td>
        <td>${s.has_secret ? sigBadge(true) : '<span class="sig-fail pill">open</span>'}</td>
      </tr>`;
    }).join('');
  }

  function reloadWebhooks() {
    setStatus('webhooks-status', 'reloading gateway webhooks...');
    fetch(BASE + '/api/v1/webhooks/reload', { method: 'POST' })
      .then(async r => {
        const data = await r.json().catch(() => ({}));
        if (!r.ok) throw new Error(data.error || r.statusText);
        renderWebhooks(data.subscriptions || []);
        setStatus('webhooks-status', `reloaded ${data.reloaded || 0} subscription(s)`);
      })
      .catch(e => {
        setStatus('webhooks-status', 'reload error: ' + e.message, true);
      });
  }

  function dispatchWebhook(event) {
    if (event) event.preventDefault();
    const eventInput = document.getElementById('webhook-event-input');
    const payloadInput = document.getElementById('webhook-payload-input');
    const eventName = (eventInput && eventInput.value || '').trim();
    if (!eventName) {
      setStatus('webhooks-status', 'webhook event is required', true);
      return;
    }
    let payload = {};
    const rawPayload = (payloadInput && payloadInput.value || '').trim();
    if (rawPayload) {
      try {
        payload = JSON.parse(rawPayload);
      } catch (e) {
        setStatus('webhooks-status', 'payload must be valid JSON', true);
        return;
      }
    }
    setStatus('webhooks-status', `dispatching ${eventName}...`);
    fetch(BASE + '/api/v1/webhooks/dispatch', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ event: eventName, payload })
    })
      .then(async r => {
        const data = await r.json().catch(() => ({}));
        if (!r.ok) throw new Error(data.error || r.statusText);
        setStatus('webhooks-status', `dispatch delivered=${data.delivered || 0} failed=${data.failed || 0}`);
      })
      .catch(e => {
        setStatus('webhooks-status', 'dispatch error: ' + e.message, true);
      });
  }

  let sseConnected = false;
  let operationAfterCursor = '';
  let selectedRunId = '';
  let runStreamAfterCursor = '';
  let directOperationAfterCursor = '';
  let operationLivePollInFlight = false;
  let operationWebSocketAfterCursor = '';
  let operationWebSocket = null;

  function eventStreamUrl() {
    if (!operationAfterCursor) return BASE + '/api/v1/events/stream';
    return BASE + '/api/v1/events/stream?after=' + encodeURIComponent(operationAfterCursor);
  }

  function selectedRunStreamUrl() {
    if (!selectedRunId) return '';
    const base = BASE + '/v1/runs/' + encodeURIComponent(selectedRunId) + '/stream';
    if (!runStreamAfterCursor) return base;
    return base + '?after=' + encodeURIComponent(runStreamAfterCursor);
  }

  function operationLiveStreamUrl() {
    const base = BASE + '/api/v1/operations/stream';
    if (!directOperationAfterCursor) return base;
    return base + '?after=' + encodeURIComponent(directOperationAfterCursor);
  }

  function operationWebSocketUrl() {
    const scheme = window.location.protocol === 'https:' ? 'wss://' : 'ws://';
    const base = scheme + window.location.host + '/api/v1/operations/ws';
    if (!operationWebSocketAfterCursor) return base;
    return base + '?after=' + encodeURIComponent(operationWebSocketAfterCursor);
  }

  function rememberOperationCursor(id, payload) {
    const cursor = id || (payload && payload.cursor) || '';
    if (cursor && cursor.startsWith('operation:')) {
      operationAfterCursor = cursor;
    }
  }

  function rememberRunStreamCursor(id, payload) {
    const cursor = id || (payload && payload.cursor) || '';
    if (cursor && cursor.startsWith('operation:')) {
      runStreamAfterCursor = cursor;
    }
  }

  function rememberDirectOperationCursor(id, payload) {
    const cursor = id || (payload && payload.cursor) || '';
    if (cursor && cursor.startsWith('operation:')) {
      directOperationAfterCursor = cursor;
    }
  }

  function rememberOperationWebSocketCursor(id, payload) {
    const cursor = id || (payload && payload.cursor) || '';
    if (cursor && cursor.startsWith('operation:')) {
      operationWebSocketAfterCursor = cursor;
    }
  }

  function handleSsePayload(raw, rememberCursor, statusId) {
    const targetStatus = statusId || 'events-status';
    const events = JSON.parse(raw);
    if (!Array.isArray(events)) {
      if (events && events.schema === 'zaion.operation_event.v1') {
        rememberCursor('', events);
        setStatus(targetStatus, events.display_text || events.kind || 'operation event');
      } else if (events && Object.prototype.hasOwnProperty.call(events, 'requested_after')) {
        setStatus(targetStatus, `resumed after ${events.requested_after || runStreamAfterCursor || directOperationAfterCursor || operationAfterCursor || 'cursor'}`);
      } else if (events && events.sink) {
        setStatus(targetStatus, `${events.sink} connected`);
      }
      return;
    }
    renderEvents(events);
    setStatus(targetStatus, `${events.length} event(s), updated ${new Date().toLocaleTimeString()}`);
  }

  function inspectRunStream(runId) {
    if (!runId) return;
    if (selectedRunId !== runId) {
      selectedRunId = runId;
      runStreamAfterCursor = '';
    }
    setStatus('events-status', `inspecting run ${short(selectedRunId, 18)} stream...`);
    pollSelectedRunStream();
    fetchRuns();
  }

  function pollSelectedRunStream() {
    const url = selectedRunStreamUrl();
    if (!url) return;
    fetch(url)
      .then(r => r.text())
      .then(text => {
        const lines = text.split('\n');
        for (const line of lines) {
          if (line.startsWith('data: ')) {
            try {
              handleSsePayload(line.slice(6), rememberRunStreamCursor);
            } catch (_) {}
          }
        }
      })
      .catch(e => {
        setStatus('events-status', 'run stream error: ' + e.message, true);
      });
  }

  function pollOperationLiveStream() {
    if (operationLivePollInFlight) return;
    operationLivePollInFlight = true;
    setStatus('operations-status', directOperationAfterCursor
      ? `polling operations after ${short(directOperationAfterCursor, 42)}...`
      : 'polling direct operation stream...');
    fetch(operationLiveStreamUrl())
      .then(r => r.text())
      .then(text => {
        let parsed = 0;
        const lines = text.split('\n');
        for (const line of lines) {
          if (line.startsWith('data: ')) {
            try {
              handleSsePayload(line.slice(6), rememberDirectOperationCursor, 'operations-status');
              parsed += 1;
            } catch (_) {}
          }
        }
        if (parsed === 0) {
          setStatus('operations-status', 'direct operation stream returned no frames');
        }
      })
      .catch(e => {
        setStatus('operations-status', 'operation stream error: ' + e.message, true);
      })
      .finally(() => {
        operationLivePollInFlight = false;
      });
  }

  function resetOperationLiveStreamCursor() {
    directOperationAfterCursor = '';
    setStatus('operations-status', 'direct operation stream cursor reset');
    pollOperationLiveStream();
  }

  function handleOperationWebSocketMessage(raw) {
    const message = JSON.parse(raw);
    const payload = message && message.payload;
    if (!message || !message.type) {
      setStatus('operations-ws-status', 'operation WebSocket ignored malformed frame', true);
      return;
    }
    if (message.type === 'operation.event') {
      rememberOperationWebSocketCursor(message.id, payload);
      setStatus('operations-ws-status', payload && (payload.display_text || payload.kind)
        ? payload.display_text || payload.kind
        : 'operation WebSocket event');
      return;
    }
    if (message.type === 'stream.contract') {
      rememberOperationWebSocketCursor('', payload);
      setStatus('operations-ws-status', `${(payload && payload.sink) || 'OperationLiveWebSocket'} connected`);
      return;
    }
    if (message.type === 'stream.resume') {
      rememberOperationWebSocketCursor('', payload);
      setStatus('operations-ws-status', `operation WebSocket resumed after ${(payload && payload.requested_after) || operationWebSocketAfterCursor || 'cursor'}`);
      return;
    }
    setStatus('operations-ws-status', `operation WebSocket ${message.type}`);
  }

  function connectOperationWebSocket() {
    if (operationWebSocket && operationWebSocket.readyState <= 1) {
      setStatus('operations-ws-status', 'operation WebSocket already connected');
      return;
    }
    if (typeof WebSocket === 'undefined') {
      setStatus('operations-ws-status', 'WebSocket unavailable in this browser', true);
      return;
    }
    setStatus('operations-ws-status', operationWebSocketAfterCursor
      ? `connecting operation WebSocket after ${short(operationWebSocketAfterCursor, 42)}...`
      : 'connecting operation WebSocket...');
    operationWebSocket = new WebSocket(operationWebSocketUrl());
    operationWebSocket.onopen = () => {
      setStatus('operations-ws-status', 'operation WebSocket connected');
    };
    operationWebSocket.onmessage = (event) => {
      try {
        handleOperationWebSocketMessage(event.data);
      } catch (e) {
        setStatus('operations-ws-status', 'operation WebSocket parse error: ' + e.message, true);
      }
    };
    operationWebSocket.onerror = () => {
      setStatus('operations-ws-status', 'operation WebSocket error', true);
    };
    operationWebSocket.onclose = () => {
      operationWebSocket = null;
      setStatus('operations-ws-status', operationWebSocketAfterCursor
        ? `operation WebSocket closed after ${short(operationWebSocketAfterCursor, 42)}`
        : 'operation WebSocket closed');
    };
  }

  function disconnectOperationWebSocket() {
    if (!operationWebSocket) {
      setStatus('operations-ws-status', 'operation WebSocket already disconnected');
      return;
    }
    const socket = operationWebSocket;
    operationWebSocket = null;
    socket.close();
    setStatus('operations-ws-status', 'operation WebSocket disconnect requested');
  }

  function connectSSE() {
    const dot = document.getElementById('sse-dot');
    dot.classList.remove('live');
    if (typeof EventSource !== 'undefined') {
      const es = new EventSource(eventStreamUrl());
      es.onopen = () => {
        sseConnected = true;
        dot.classList.add('live');
        setStatus('events-status', 'SSE connected');
      };
      es.onmessage = (e) => {
        try {
          const events = JSON.parse(e.data);
          renderEvents(events);
        } catch (_) {}
      };
      es.addEventListener('ledger.snapshot', (e) => {
        try {
          const events = JSON.parse(e.data);
          renderEvents(events);
        } catch (_) {}
      });
      es.addEventListener('stream.contract', (e) => {
        try {
          const contract = JSON.parse(e.data);
          setStatus('events-status', `${contract.sink || 'stream'} connected`);
        } catch (_) {}
      });
      es.addEventListener('stream.resume', (e) => {
        try {
          const resume = JSON.parse(e.data);
          setStatus('events-status', `resumed after ${resume.requested_after || 'cursor'}`);
        } catch (_) {}
      });
      es.addEventListener('operation.event', (e) => {
        try {
          const operation = JSON.parse(e.data);
          rememberOperationCursor(e.lastEventId, operation);
          setStatus('events-status', operation.display_text || operation.kind || 'operation event');
        } catch (_) {}
      });
      es.onerror = () => {
        sseConnected = false;
        dot.classList.remove('live');
        setStatus('events-status', 'SSE error; falling back to polling', true);
        es.close();
        pollEvents();
      };
    } else {
      pollEvents();
    }
  }

  function pollEvents() {
    const dot = document.getElementById('sse-dot');
    fetch(eventStreamUrl())
      .then(r => r.text())
      .then(text => {
        const lines = text.split('\n');
        for (const line of lines) {
          if (line.startsWith('data: ')) {
            try {
              const events = JSON.parse(line.slice(6));
              if (!Array.isArray(events)) {
                if (events && events.schema === 'zaion.operation_event.v1') {
                  rememberOperationCursor('', events);
                  setStatus('events-status', events.display_text || events.kind || 'operation event');
                } else if (events && Object.prototype.hasOwnProperty.call(events, 'requested_after')) {
                  setStatus('events-status', `resumed after ${events.requested_after || operationAfterCursor || 'cursor'}`);
                } else if (events && events.sink) {
                  setStatus('events-status', `${events.sink} connected`);
                }
                continue;
              }
              renderEvents(events);
              dot.classList.add('live');
              setStatus('events-status', `${events.length} event(s), updated ${new Date().toLocaleTimeString()}`);
            } catch (_) {}
          }
        }
      })
      .catch(e => {
        dot.classList.remove('live');
        setStatus('events-status', 'poll error: ' + e.message, true);
      });
  }

  function renderEvents(events) {
    const tbody = document.getElementById('events-body');
    if (!Array.isArray(events) || events.length === 0) {
      text('event-count', '0');
      tbody.innerHTML = '<tr><td colspan="4" class="empty">no events in ledger</td></tr>';
      setStatus('events-status', 'no events yet');
      return;
    }
    text('event-count', String(Math.min(events.length, 50)));
    setStatus('events-status', `${events.length} event(s), updated ${new Date().toLocaleTimeString()}`);
    tbody.innerHTML = events.slice(0, 50).map(ev => {
      const sigOk = ev.sig_valid === true ? true : ev.sig_valid === false ? false : null;
      return `<tr>
        <td title="${escapeHtml(ev.created_at || '')}">${escapeHtml(timeAgo(ev.created_at))}</td>
        <td title="${escapeHtml(ev.principal_id || '')}"><code>${escapeHtml(short(ev.principal_id, 16))}</code></td>
        <td>${escapeHtml(ev.event_type || '?')}</td>
        <td>${sigBadge(sigOk)}</td>
      </tr>`;
    }).join('');
  }

  fetchProcesses();
  fetchRuns();
  fetchWebhooks();
  connectSSE();
  document.getElementById('run-submit-form').addEventListener('submit', submitRun);
  document.getElementById('webhook-dispatch-form').addEventListener('submit', dispatchWebhook);
  document.getElementById('webhook-reload-button').addEventListener('click', reloadWebhooks);
  document.getElementById('operation-live-button').addEventListener('click', pollOperationLiveStream);
  document.getElementById('operation-ws-button').addEventListener('click', connectOperationWebSocket);
  document.getElementById('operation-ws-disconnect-button').addEventListener('click', disconnectOperationWebSocket);
  document.getElementById('operation-cursor-reset-button').addEventListener('click', resetOperationLiveStreamCursor);

  setInterval(fetchProcesses, 3000);
  setInterval(fetchRuns, 3000);
  setInterval(fetchWebhooks, 3000);
  setInterval(pollEvents, 3000);
  setInterval(pollSelectedRunStream, 3000);
  setInterval(pollOperationLiveStream, 3000);
})();
</script>
</body>
</html>"#;
