// ============================================================
// 开机动画管理器 · WebUI
// 纯 JS 动态构建 DOM，HTML 仅保留空壳 + 页面切换样式
// ============================================================

import "mdui/mdui.css";
import "mdui";

import "@mdui/icons/info.js";
import "@mdui/icons/apps.js";
import "@mdui/icons/add.js";
import "@mdui/icons/folder.js";
import "@mdui/icons/folder-zip.js";
import "@mdui/icons/chevron-right.js";
import "@mdui/icons/check-circle.js";
import "@mdui/icons/error.js";
import "@mdui/icons/power.js";

// ==================== 常量 ====================
const MODULE_ID = "bootanimation_manager";
const MODULE_DIR = `/data/adb/modules/${MODULE_ID}`;
const UTILS_CMD = `cd ${MODULE_DIR} && MODDIR=${MODULE_DIR} ./utils`;

const ksu = window.ksu || window.ksuwebui || {};

const CLR = {
  selectedBg: "#3b82f6",
  selectedFg: "#ffffff",
  selectedBgLight: "rgba(59,130,246,0.15)",
};

// ==================== DOM 辅助 ====================
function el(tag, props = {}, children = []) {
  const node = document.createElement(tag);
  for (const [key, val] of Object.entries(props || {})) {
    if (val == null || val === false) continue;
    if (key === "style" && typeof val === "object") {
      Object.assign(node.style, val);
    } else if (key === "class") {
      node.className = val;
    } else if (key === "html") {
      node.innerHTML = val;
    } else if (key === "text") {
      node.textContent = val;
    } else if (key === "on" && typeof val === "object") {
      for (const [ev, fn] of Object.entries(val)) {
        if (typeof fn === "function") node.addEventListener(ev, fn);
      }
    } else if (key === "dataset" && typeof val === "object") {
      for (const [dk, dv] of Object.entries(val)) node.dataset[dk] = dv;
    } else if (key in node && typeof node[key] !== "function") {
      try { node[key] = val; } catch (_) { node.setAttribute(key, val); }
    } else {
      node.setAttribute(key, val);
    }
  }
  for (const child of [].concat(children)) {
    if (child == null || child === false) continue;
    node.appendChild(typeof child === "string" ? document.createTextNode(child) : child);
  }
  return node;
}

// ==================== 工具 ====================
function toast(msg) {
  if (ksu.toast) { ksu.toast(msg); return; }
  const t = el("div", {
    text: msg,
    style: {
      position: "fixed", bottom: "90px", left: "50%",
      transform: "translateX(-50%)", background: "#333", color: "#fff",
      padding: "10px 20px", borderRadius: "24px", fontSize: "0.85rem",
      zIndex: 9999, boxShadow: "0 4px 12px rgba(0,0,0,0.4)",
    },
  });
  document.body.appendChild(t);
  setTimeout(() => t.remove(), 2400);
}

function shAsync(cmd) {
  return new Promise((resolve) => {
    if (!ksu.exec) { resolve({ errno: -1, stdout: "", stderr: "KSU API 不可用" }); return; }
    const cb = `_cb_${Date.now()}_${Math.random().toString(36).slice(2)}`;
    window[cb] = (errno, stdout, stderr) => {
      delete window[cb];
      resolve({ errno, stdout: stdout || "", stderr: stderr || "" });
    };
    try { ksu.exec(cmd, cb); }
    catch (e) { delete window[cb]; resolve({ errno: -1, stdout: "", stderr: e.message }); }
  });
}

async function shRetry(cmd, validate = (r) => parseResult(r.stdout) !== null, attempts = 5, delay = 200) {
  let r;
  for (let i = 0; i < attempts; i++) {
    r = await shAsync(cmd);
    if (validate(r)) return r;
    if (i < attempts - 1) await sleep(delay);
  }
  return r;
}

function shQuote(s) {
  return "'" + String(s).replace(/'/g, "'\\''") + "'";
}

function parseResult(raw) {
  if (!raw || !raw.trim()) return null;
  try { return JSON.parse(raw); } catch (_) {}
  for (const line of raw.split("\n")) {
    const t = line.trim();
    if (t.startsWith("{")) { try { return JSON.parse(t); } catch (_) {} }
  }
  return null;
}

function sleep(ms) { return new Promise(r => setTimeout(r, ms)); }

// ==================== 样式 ====================
const C = {
  card: { width: "100%", marginBottom: "16px", padding: "16px" },
  row: { display: "flex", justifyContent: "space-between", padding: "6px 0", fontSize: "0.85rem" },
  muted: { opacity: 0.6 },
  mono: { fontFamily: "monospace", fontSize: "0.8rem" },
  badge: (bg, fg) => ({
    background: bg, color: fg, padding: "2px 8px",
    borderRadius: "6px", fontSize: "0.7rem", fontWeight: 600,
  }),
  wrap: { padding: "16px", maxWidth: "720px", margin: "0 auto" },
  title: { fontSize: "1.3rem", fontWeight: 600, marginBottom: "16px" },
  gap: { display: "flex", gap: "8px", flexWrap: "wrap" },
  fileRow: { display: "flex", alignItems: "center", padding: "12px 8px", cursor: "pointer", borderRadius: "8px" },
  overlay: {
    position: "fixed", inset: 0, background: "rgba(0,0,0,0.6)", zIndex: 9998,
    display: "flex", alignItems: "center", justifyContent: "center", padding: "24px",
  },
  dialog: {
    background: "rgb(var(--mdui-color-surface-container-high, 236, 233, 230))",
    color: "rgb(var(--mdui-color-on-surface, 0, 0, 0))",
    borderRadius: "16px", padding: "20px", width: "100%", maxWidth: "340px",
    boxShadow: "0 8px 32px rgba(0,0,0,0.5)",
  },
  input: {
    width: "100%", boxSizing: "border-box", padding: "10px 12px", borderRadius: "8px",
    border: "1px solid rgb(var(--mdui-color-outline, 121, 116, 126))",
    background: "rgb(var(--mdui-color-surface-container-lowest, 255, 255, 255))",
    color: "rgb(var(--mdui-color-on-surface, 0, 0, 0))",
    fontSize: "0.95rem", fontFamily: "monospace", outline: "none",
  },
};

// ==================== 电源菜单 ====================
function showPowerMenu() {
  const items = [
    { label: "重启", cmd: "reboot" },
    { label: "软重启", cmd: "ksud soft-reboot" },
    { label: "重启到 Recovery", cmd: "reboot recovery" },
    { label: "重启到 Bootloader", cmd: "reboot bootloader" },
    { label: "重启到 EDL", cmd: "reboot edl" },
    { label: "重启到 Download", cmd: "reboot download" },
  ];

  const overlay = el("div", {
    style: { ...C.overlay, alignItems: "flex-end", padding: "0" },
    on: { click: (e) => { if (e.target === overlay) overlay.remove(); } },
  }, [
    el("div", {
      style: {
        background: "rgb(var(--mdui-color-surface-container-high, 236, 233, 230))",
        color: "rgb(var(--mdui-color-on-surface, 0, 0, 0))",
        width: "100%",
        borderRadius: "20px 20px 0 0",
        padding: "12px 0 max(20px, env(safe-area-inset-bottom)) 0",
        boxShadow: "0 -4px 24px rgba(0,0,0,0.3)",
      },
    }, [
      el("div", {
        style: {
          width: "36px", height: "4px", borderRadius: "2px",
          background: "var(--mdui-color-outline-variant, #cac4d0)",
          margin: "0 auto 16px",
        },
      }),
      el("div", { style: { fontSize: "0.85rem", fontWeight: 600, opacity: 0.6, padding: "0 20px 8px" }, text: "电源选项" }),
      ...items.map(({ label, cmd }) =>
        el("div", {
          style: {
            display: "flex", alignItems: "center",
            padding: "15px 20px", cursor: "pointer", fontSize: "0.92rem",
            WebkitTapHighlightColor: "transparent",
          },
          on: {
            click: () => {
              overlay.remove();
              executePowerCommand(cmd, label);
            },
          },
        }, [el("span", { text: label })])
      ),
    ]),
  ]);

  document.body.appendChild(overlay);
}

async function executePowerCommand(cmd, label) {
  toast(`正在执行：${label}...`);
  await sleep(300);
  shAsync(cmd);
  // 重启命令执行后设备会重启，WebView 不会返回，所以不刷新 UI
}

// ==================== 状态页 ====================
async function refreshStatus() {
  const box = document.getElementById("status-content");
  box.innerHTML = '<div style="text-align:center;padding:20px;opacity:0.5;">读取中...</div>';
  const { stdout } = await shRetry(`${UTILS_CMD} status --json`);
  const res = parseResult(stdout);
  if (!res || !res.success) {
    box.innerHTML = '<div style="text-align:center;padding:20px;opacity:0.5;">无法读取状态</div>';
    return;
  }
  const d = res.data;
  const configId = d.config_animation_id || d.animation_id || null;
  const nowId = d.now_animation_id || null;
  const mounted = d.main_mounted;

  let mode, modeBadge;
  if (!mounted) {
    mode = "未挂载";
    modeBadge = C.badge("#713f12", "#fde047");
  } else if (nowId && configId && nowId !== configId) {
    mode = "临时 / 一次性";
    modeBadge = C.badge("#1e3a8a", "#93c5fd");
  } else {
    mode = "持久化";
    modeBadge = C.badge("#166534", "#86efac");
  }

  const format = d.system_format === "coloros"
    ? "ColorOS"
    : d.system_path ? "AOSP" : "未检测到";

  const enableSwitch = el("mdui-switch", {
    id: "switch-enable",
    on: { change: (e) => {
      const action = e.target.checked ? "enable" : "disable";
      shAsync(`${UTILS_CMD} ${action} --json`).then(({ stdout: s }) => {
        const r = parseResult(s);
        toast(r && r.success ? `已${action === "enable" ? "启用" : "禁用"}开机自动挂载` : "操作失败");
        refreshStatus();
      });
    } },
  });
  enableSwitch.checked = !!d.enabled;

  const rows = [
    ["挂载模式", el("span", { style: modeBadge }, [mode])],
    ["开机自动挂载", enableSwitch],
    ["系统格式", format],
    ["系统路径", el("span", {
      style: { ...C.mono, wordBreak: "break-all", whiteSpace: "pre-wrap" },
      text: d.system_path || "(未检测到)",
    })],
    ["当前生效动画", el("span", { style: C.mono, text: nowId || "(未挂载)" })],
    ["持久化默认", el("span", { style: C.mono, text: configId || "(未设置)" })],
    ["界面语言", d.language || "默认（跟随系统）"],
    ["模块文件", d.module_file_exists ? "存在" : "缺失"],
    ["挂载源", d.main_sources && d.main_sources.length
      ? el("span", { style: { ...C.mono, wordBreak: "break-all" }, text: d.main_sources.join(", ") })
      : "无"],
  ];

  box.innerHTML = "";
  rows.forEach(([k, v]) => {
    box.appendChild(el("div", { style: C.row }, [
      el("span", { style: C.muted, text: k }),
      v,
    ]));
  });
  if (d.dark_mounted) {
    box.appendChild(el("div", { style: C.row }, [
      el("span", { style: C.muted, text: "Dark 模式" }),
      el("span", { style: C.badge("#166534", "#86efac") }, ["已挂载"]),
    ]));
  }
}

// ==================== 动画列表页 ====================
async function refreshList() {
  const el_ = document.getElementById("anim-list");
  el_.innerHTML = '<div style="text-align:center;padding:20px;opacity:0.5;">加载中...</div>';

  const [listRes, infoRes, statusRes] = await Promise.all([
    shRetry(`${UTILS_CMD} list --json`),
    shRetry(`${UTILS_CMD} info --json`),
    shRetry(`${UTILS_CMD} status --json`),
  ]);
  const list = parseResult(listRes.stdout);
  const info = parseResult(infoRes.stdout);
  const status = parseResult(statusRes.stdout);

  if (!list || !list.success || !list.data || !list.data.animations || list.data.animations.length === 0) {
    el_.innerHTML = '<div style="text-align:center;padding:24px;opacity:0.4;font-size:0.85rem;">没有已安装的动画</div>';
    return;
  }

  const metaMap = {};
  if (info && info.success && info.data && Array.isArray(info.data.info)) {
    for (const m of info.data.info) if (m && m.id) metaMap[m.id] = m;
  }

  const configId = status && status.data ? (status.data.config_animation_id || status.data.animation_id || null) : null;
  const nowId = status && status.data ? (status.data.now_animation_id || null) : null;

  const all = list.data.animations;
  const ordered = [];
  const added = new Set();
  if (configId) {
    const c = all.find(a => a.id === configId);
    if (c) { ordered.push(c); added.add(c.id); }
  }
  if (nowId && !added.has(nowId)) {
    const n = all.find(a => a.id === nowId);
    if (n) { ordered.push(n); added.add(n.id); }
  }
  all.forEach(a => { if (!added.has(a.id)) ordered.push(a); });

  el_.innerHTML = "";
  ordered.forEach(item => {
    const m = metaMap[item.id] || {};
    const isDefault = configId === item.id;
    const isNow = nowId === item.id;

    const versionNode = m.version
      ? (m.version_code
          ? el("span", {}, [
              m.version, " ",
              el("span", { style: { opacity: 0.5 }, text: `(${m.version_code})` }),
            ])
          : m.version)
      : null;

    const metaRows = [
      ["作者", m.author],
      ["版本", versionNode],
      ["描述", m.desc],
      ["ID", item.id],
    ].filter(([, v]) => v !== undefined && v !== null && v !== "");

    const meta = el("div", {
      style: {
        marginTop: "10px", padding: "10px",
        background: "var(--mdui-color-surface-variant)",
        borderRadius: "8px", fontSize: "0.8rem",
      },
    }, metaRows.length
      ? metaRows.map(([k, v]) => el("div", { style: { display: "flex", gap: "8px", padding: "3px 0" } }, [
          el("span", { style: { opacity: 0.6, minWidth: "64px", flexShrink: 0 }, text: k }),
          el("span", { style: { wordBreak: "break-word" } }, [v]),
        ]))
      : [el("div", { style: C.muted, text: "无更多元数据" })]);

    const titleRow = el("div", {
      style: {
        display: "flex", alignItems: "center", gap: "6px",
        flexWrap: "wrap", cursor: "pointer", userSelect: "none",
      },
      on: { click: () => {
        const isHidden = meta.style.display === "none";
        meta.style.display = isHidden ? "block" : "none";
        const icon = card.querySelector(".chevron");
        if (icon) icon.style.transform = isHidden ? "rotate(90deg)" : "";
      } },
    }, [
      el("mdui-icon-chevron-right", { class: "chevron", style: {
        fontSize: "18px", transition: "transform 0.15s", transform: "rotate(90deg)",
      } }),
      el("span", { style: { fontWeight: 500 }, text: item.name || item.id }),
    ]);
    if (isDefault) titleRow.appendChild(el("span", { style: C.badge("#7c3aed", "#e9d5ff") }, ["默认"]));
    else if (isNow) titleRow.appendChild(el("span", { style: C.badge("#166534", "#86efac") }, ["当前挂载"]));

    const actionBtn = isDefault
      ? el("mdui-button", { variant: "filled", disabled: true, style: { fontSize: "0.78rem" }, text: "使用中" })
      : isNow
        ? el("mdui-button", { variant: "filled", disabled: true, style: { fontSize: "0.78rem" }, text: "当前" })
        : el("mdui-button", { variant: "filled", style: { fontSize: "0.78rem" }, text: "使用", on: { click: () => useAnimation(item.id) } });

    const actionRow = el("div", {
      style: { display: "flex", gap: "6px", flexWrap: "wrap", marginTop: "12px" },
    }, [
      actionBtn,
      el("mdui-button", { variant: "outlined", style: { fontSize: "0.78rem" }, text: "预览", on: { click: () => previewAnimation(item.id) } }),
      el("mdui-button", { variant: "outlined", style: { fontSize: "0.78rem" }, text: "用一次", on: { click: () => onceApply(item.id) } }),
      el("mdui-button", {
        variant: "text",
        style: { fontSize: "0.78rem", color: "#f87171" },
        text: "删除",
        on: { click: () => removeAnimation(item.id, item.name || item.id) },
      }),
    ]);

    const card = el("mdui-card", { style: { width: "100%", marginBottom: "12px" } }, [
      el("div", { style: { padding: "14px" } }, [titleRow, meta, actionRow]),
    ]);
    el_.appendChild(card);
  });
}

// ==================== 操作 ====================
async function useAnimation(id) {
  toast(`正在设置 ${id}...`);
  const { stdout, stderr } = await shAsync(`${UTILS_CMD} use ${shQuote(id)} --json`);
  const res = parseResult(stdout);
  if (res && res.success) { toast(`已使用：${id}`); refreshStatus(); refreshList(); }
  else toast(res?.error || stderr || "设置失败");
}

async function onceApply(id) {
  toast(`正在设置一次性动画 ${id}...`);
  const { stdout, stderr } = await shAsync(`${UTILS_CMD} once_apply ${shQuote(id)} --json`);
  const res = parseResult(stdout);
  if (res && res.success) { toast(`下次开机使用 ${id}，之后恢复默认`); refreshStatus(); refreshList(); }
  else toast(res?.error || stderr || "设置失败");
}

async function removeAnimation(id, displayName) {
  const ok = await showConfirmDialog({
    title: "删除动画",
    message: `确定要删除「${displayName || id}」吗？此操作不可撤销。`,
    confirmText: "删除",
    danger: true,
  });
  if (!ok) return;
  toast(`正在删除 ${id}...`);
  const { stdout, stderr } = await shAsync(`${UTILS_CMD} remove ${shQuote(id)} --json`);
  const res = parseResult(stdout);
  if (res && res.success) { toast(`已删除：${id}`); refreshStatus(); refreshList(); }
  else toast(res?.error || stderr || "删除失败");
}

async function applyCurrent() {
  toast("正在应用...");
  const { stdout, stderr } = await shAsync(`${UTILS_CMD} apply --json`);
  const res = parseResult(stdout);
  if (res && res.success) toast(res.data?.message || "应用成功");
  else toast(res?.error || stderr || "应用失败");
  refreshStatus(); refreshList();
}

async function revert() {
  toast("正在卸载...");
  const { stdout, stderr } = await shAsync(`${UTILS_CMD} revert --json`);
  const res = parseResult(stdout);
  if (res && res.success) toast(res.data?.message || "已卸载");
  else toast(res?.error || stderr || "卸载失败");
  refreshStatus(); refreshList();
}

async function check() {
  const { stdout } = await shRetry(`${UTILS_CMD} check --json`);
  const res = parseResult(stdout);
  if (res && res.success && res.data) showCheckDialog(res.data);
  else toast("检查失败");
}

function shortHash(h) { return h ? `${h.slice(0, 16)}…` : "无"; }

function showCheckDialog(d) {
  const lines = [
    ["状态", d.active ? "✅ 已生效" : "❌ 未完全生效"],
    ["模块已挂载", d.mounted ? "是" : "否"],
    ["当前生效 ID", d.now_animation_id || "(未挂载)"],
    ["持久化默认 ID", d.config_animation_id || "(未设置)"],
    ["已启用", d.enabled ? "是" : "否"],
    ["模块动画存在", d.module_exists ? "是" : "否"],
    ["系统路径存在", d.system_exists ? "是" : "否"],
    ["模块哈希", shortHash(d.module_hash)],
    ["系统哈希", shortHash(d.system_hash)],
  ];
  const body = el("div", { style: { fontSize: "0.82rem" } }, lines.map(([k, v]) => el("div", {
    style: { display: "flex", justifyContent: "space-between", padding: "7px 0", borderBottom: "1px solid var(--mdui-color-outline-variant)" },
  }, [
    el("span", { style: C.muted, text: k }),
    el("span", { style: C.mono, text: String(v) }),
  ])));

  const overlay = el("div", { style: C.overlay, on: { click: (e) => { if (e.target === overlay) overlay.remove(); } } }, [
    el("div", { style: C.dialog }, [
      el("div", { style: { display: "flex", alignItems: "center", gap: "8px", marginBottom: "12px" } }, [
        el(d.active ? "mdui-icon-check-circle" : "mdui-icon-error", {
          style: { fontSize: "22px", color: d.active ? "#4ade80" : "#f87171" },
        }),
        el("span", { style: { fontSize: "1.05rem", fontWeight: 600 }, text: "生效诊断" }),
      ]),
      body,
      el("div", { style: { fontSize: "0.82rem", opacity: 0.75, marginTop: "12px", marginBottom: "16px" }, text: d.message }),
      el("div", { style: { display: "flex", justifyContent: "flex-end" } }, [
        el("mdui-button", { variant: "filled", text: "知道了", on: { click: () => overlay.remove() } }),
      ]),
    ]),
  ]);
  document.body.appendChild(overlay);
}

// ==================== 通用确认对话框 ====================
function showConfirmDialog({ title, message, confirmText = "确定", cancelText = "取消", danger = false }) {
  return new Promise((resolve) => {
    const overlay = el("div", { style: C.overlay, on: { click: (e) => { if (e.target === overlay) { overlay.remove(); resolve(false); } } } }, [
      el("div", { style: C.dialog }, [
        el("div", { style: { fontSize: "1.05rem", fontWeight: 600, marginBottom: "10px" }, text: title }),
        el("div", { style: { fontSize: "0.85rem", opacity: 0.8, marginBottom: "18px", lineHeight: 1.5, wordBreak: "break-word" }, text: message }),
        el("div", { style: { display: "flex", justifyContent: "flex-end", gap: "8px" } }, [
          el("mdui-button", { variant: "text", text: cancelText, on: { click: () => { overlay.remove(); resolve(false); } } }),
          el("mdui-button", {
            variant: "filled",
            text: confirmText,
            style: danger ? { background: "#dc2626", color: "#fff" } : {},
            on: { click: () => { overlay.remove(); resolve(true); } },
          }),
        ]),
      ]),
    ]);
    document.body.appendChild(overlay);
  });
}

// ==================== 预览功能 ====================
function showPreviewDialog(id) {
  return new Promise((resolve) => {
    const input = el("input", {
      id: "preview-seconds", type: "number", inputmode: "numeric", value: "0", min: "0",
      style: C.input,
      on: {
        keydown: (e) => {
          if (e.key === "Enter") { e.preventDefault(); confirm(); }
          if (e.key === "Escape") resolve(null);
        },
      },
    });
    const overlay = el("div", { style: C.overlay, on: { click: (e) => { if (e.target === overlay) resolve(null); } } }, [
      el("div", { style: C.dialog }, [
        el("div", { style: { fontSize: "1.05rem", fontWeight: 600, marginBottom: "8px" }, text: "预览动画" }),
        el("div", { style: { fontSize: "0.8rem", opacity: 0.6, marginBottom: "14px", wordBreak: "break-all", fontFamily: "monospace" }, text: id }),
        el("div", { style: { fontSize: "0.82rem", opacity: 0.75, marginBottom: "8px" }, text: "输入预览秒数（0 = 无限，按音量键退出）" }),
        input,
        el("div", { style: { display: "flex", justifyContent: "flex-end", gap: "8px", marginTop: "18px" } }, [
          el("mdui-button", { variant: "text", text: "取消", on: { click: () => resolve(null) } }),
          el("mdui-button", { variant: "filled", text: "开始预览", on: { click: confirm } }),
        ]),
      ]),
    ]);
    document.body.appendChild(overlay);
    setTimeout(() => { input.focus(); input.select(); }, 50);

    function confirm() {
      const raw = input.value.trim();
      let num = parseInt(raw, 10);
      if (isNaN(num) || num < 0) num = 0;
      resolve(num);
    }
  });
}

async function previewAnimation(id) {
  const seconds = await showPreviewDialog(id);
  if (seconds === null) return;

  toast(`正在挂载 ${id}...`);
  const { stdout, stderr } = await shAsync(`${UTILS_CMD} apply --temp ${shQuote(id)} --json`);
  const res = parseResult(stdout);
  if (!res || !res.success) {
    toast(res?.error || stderr || "临时挂载失败，已取消预览");
    return;
  }

  toast("3 秒后开始预览...");
  await sleep(1000); toast("2...");
  await sleep(1000); toast("1...");
  await sleep(1000);

  toast(`正在预览 ${id}（${seconds === 0 ? "无限，按音量键退出" : seconds + " 秒"}）`);
  shAsync(`cd ${shQuote(MODULE_DIR)} && MODDIR=${shQuote(MODULE_DIR)} sh action.sh ${shQuote(String(seconds))}`)
    .then(() => shAsync(`${UTILS_CMD} apply --json`))
    .then(() => refreshStatus());
}

// ==================== 安装页文件选择 ====================
const FilePicker = {
  currentPath: "/sdcard",
  selected: null,

  init() {
    document.getElementById("btn-confirm").addEventListener("click", () => this.confirm());
    this.updateConfirm();
    this.load(this.currentPath);
  },

  updateConfirm() {
    const btn = document.getElementById("btn-confirm");
    btn.disabled = !this.selected;
    btn.textContent = "安装选中文件";
    const pathEl = document.getElementById("install-selected-path");
    if (pathEl) {
      if (this.selected) {
        pathEl.textContent = `已选择：${this.selected}`;
        pathEl.style.opacity = "1";
        pathEl.style.background = CLR.selectedBgLight;
        pathEl.style.color = CLR.selectedBg;
        pathEl.style.border = `1px solid ${CLR.selectedBg}`;
      } else {
        pathEl.textContent = "未选择文件";
        pathEl.style.opacity = "0.6";
        pathEl.style.background = "transparent";
        pathEl.style.color = "inherit";
        pathEl.style.border = "1px dashed var(--mdui-color-outline, #79747e)";
      }
    }
  },

  async load(path) {
    const body = document.getElementById("install-body");
    const pathEl = document.getElementById("install-path");
    body.innerHTML = '<div style="text-align:center;padding:20px;opacity:0.5;">读取目录...</div>';
    pathEl.textContent = path;

    const { stdout, errno } = await shRetry(
      `LC_ALL=C ls -1pL ${shQuote(path)} 2>/dev/null`,
      r => r.errno === 0
    );
    if (errno !== 0 || !stdout.trim()) {
      body.innerHTML = '<div style="text-align:center;padding:20px;opacity:0.5;">目录为空或无法读取</div>';
      this.currentPath = path;
      return;
    }
    this.currentPath = path;

    const entries = stdout.split("\n").map(s => s.trim()).filter(Boolean);
    const dirs = entries.filter(n => n.endsWith("/"));
    const files = entries.filter(n => !n.endsWith("/"));
    const visible = arr => arr.filter(n => !n.startsWith("."));

    const frag = document.createDocumentFragment();

    if (path !== "/" && path !== "") {
      const parent = path.replace(/\/+$/, "").split("/").slice(0, -1).join("/") || "/";
      frag.appendChild(el("div", {
        class: "file-row", style: C.fileRow,
        on: { click: () => {
          this.selected = null;
          this.updateConfirm();
          this.load(parent);
        } },
      }, [
        el("mdui-icon-folder", { style: { marginRight: "10px", color: "var(--mdui-color-outline, #79747e)" } }),
        el("span", { style: { fontSize: "0.88rem", flex: 1, wordBreak: "break-all", opacity: 0.7 }, text: ".." }),
      ]));
    }

    visible(dirs).forEach(d => {
      const name = d.slice(0, -1);
      frag.appendChild(el("div", {
        class: "file-row", style: C.fileRow,
        on: { click: () => {
          this.selected = null;
          this.updateConfirm();
          this.load(`${this.currentPath.replace(/\/+$/, "")}/${name}`);
        } },
      }, [
        el("mdui-icon-folder", { style: { marginRight: "10px", color: "var(--mdui-color-primary, #6750a4)" } }),
        el("span", { style: { fontSize: "0.88rem", flex: 1, wordBreak: "break-all" }, text: name }),
      ]));
    });

    visible(files).forEach(f => {
      if (!f.toLowerCase().endsWith(".zip")) return;
      const fullPath = `${this.currentPath.replace(/\/+$/, "")}/${f}`;
      const isSelected = this.selected === fullPath;

      const row = el("div", {
        class: "file-row",
        style: {
          ...C.fileRow,
          background: isSelected ? CLR.selectedBg : "transparent",
          color: isSelected ? CLR.selectedFg : "inherit",
        },
        on: { click: (e) => {
          e.currentTarget.parentElement.querySelectorAll(".file-row").forEach(r => {
            r.style.background = "transparent";
            r.style.color = "inherit";
          });
          e.currentTarget.style.background = CLR.selectedBg;
          e.currentTarget.style.color = CLR.selectedFg;
          this.selected = fullPath;
          this.updateConfirm();
        } },
      }, [
        el("mdui-icon-folder-zip", { style: {
          marginRight: "10px",
          color: isSelected ? CLR.selectedFg : "var(--mdui-color-tertiary, #7d5260)",
        } }),
        el("span", { style: { fontSize: "0.88rem", flex: 1, wordBreak: "break-all" }, text: f }),
        el("span", { style: { fontSize: "0.72rem", opacity: isSelected ? 0.85 : 0.5 }, text: "ZIP" }),
      ]);
      frag.appendChild(row);
    });

    body.innerHTML = "";
    if (!frag.children.length) {
      body.innerHTML = '<div style="text-align:center;padding:20px;opacity:0.5;">此目录下没有 zip 文件</div>';
    } else {
      body.appendChild(frag);
    }
  },

  async confirm() {
    if (!this.selected) return;
    const zipPath = this.selected;
    toast("正在安装，请稍候...");
    const { stdout, stderr } = await shAsync(`${UTILS_CMD} install ${shQuote(zipPath)} --json`);
    const res = parseResult(stdout);

    if (res && res.success && res.data) {
      const { installed = [], updated = [], skipped = [] } = res.data;
      const parts = [];
      if (installed.length) parts.push(`安装 ${installed.length} 个`);
      if (updated.length) parts.push(`更新 ${updated.length} 个`);
      if (skipped.length) parts.push(`跳过 ${skipped.length} 个`);
      toast(parts.length ? `完成：${parts.join("，")}` : "安装完成");
      if (skipped.length) setTimeout(() => toast(`跳过：${skipped.join(", ")}`), 800);
    } else {
      toast(res?.error || stderr || "安装失败");
    }
    this.selected = null;
    this.updateConfirm();
    refreshStatus(); refreshList();
    this.load(this.currentPath);
  },
};

// ==================== 页面切换 ====================
function switchPage(name) {
  document.querySelectorAll(".page").forEach(p => p.classList.remove("active"));
  const target = document.getElementById(`page-${name}`);
  if (target) target.classList.add("active");
  if (name === "status") refreshStatus();
  if (name === "list") refreshList();
  if (name === "install") FilePicker.load(FilePicker.currentPath);
}

// ==================== 构建应用 ====================
function buildApp() {
  const app = el("div", { id: "app" });

  // 状态页标题行（右侧电源按钮）
  const statusTitle = el("div", {
    style: {
      display: "flex", justifyContent: "space-between", alignItems: "center",
      marginBottom: "16px",
    },
  }, [
    el("div", { style: C.title, text: "状态" }),
    el("mdui-button", {
      variant: "text",
      style: { minWidth: "auto", padding: "8px 10px" },
      title: "电源菜单",
      on: { click: showPowerMenu },
    }, [
      el("mdui-icon-power", { style: { fontSize: "24px" } }),
    ]),
  ]);

  app.appendChild(el("div", { class: "page active", id: "page-status" }, [
    el("div", { style: C.wrap }, [
      statusTitle,
      el("mdui-card", { style: { width: "100%", marginBottom: "16px" } }, [
        el("div", { style: { padding: "16px" }, id: "status-content", text: "读取中..." }),
      ]),
      el("mdui-card", { style: C.card }, [
        el("div", { style: { fontWeight: 600, fontSize: "0.95rem", marginBottom: "12px" }, text: "操作" }),
        el("div", { style: C.gap }, [
          el("mdui-button", { variant: "filled", text: "应用当前配置", on: { click: applyCurrent } }),
          el("mdui-button", { variant: "outlined", text: "卸载动画", on: { click: revert } }),
          el("mdui-button", { variant: "text", id: "btn-check", text: "检查生效状态", on: { click: check } }),
        ]),
      ]),
    ]),
  ]));

  // 动画列表页
  app.appendChild(el("div", { class: "page", id: "page-list" }, [
    el("div", { style: C.wrap }, [
      el("div", { style: C.title, text: "已安装动画" }),
      el("div", { id: "anim-list", text: "加载中..." }),
    ]),
  ]));

  // 安装页
  app.appendChild(el("div", { class: "page", id: "page-install" }, [
    el("div", { style: C.wrap }, [
      el("div", { style: C.title, text: "安装动画包" }),
      el("div", { id: "install-path", style: {
        fontSize: "0.78rem", fontFamily: "monospace", opacity: 0.7,
        wordBreak: "break-all", marginBottom: "8px",
      }, text: "/sdcard" }),
      el("mdui-button", {
        variant: "filled", id: "btn-confirm",
        style: { width: "100%" }, disabled: true, text: "安装选中文件",
      }),
      el("div", { id: "install-selected-path", style: {
        fontSize: "0.78rem", fontFamily: "monospace", opacity: 0.6,
        wordBreak: "break-all", marginTop: "8px",
        padding: "8px 10px", borderRadius: "8px",
        border: "1px dashed var(--mdui-color-outline, #79747e)",
        transition: "all 0.15s",
      }, text: "未选择文件" }),
      el("div", { id: "install-body", style: { marginTop: "12px" } }, [
        el("div", { style: { textAlign: "center", padding: "20px", opacity: 0.5 }, text: "加载中..." }),
      ]),
    ]),
  ]));

  document.body.appendChild(app);

  // 底部导航栏
  document.body.appendChild(el("mdui-navigation-bar", {
    value: "status", id: "bottom-nav", "label-visibility": "labeled",
    on: { change: (e) => switchPage(e.target.value) },
  }, [
    el("mdui-navigation-bar-item", { value: "status" }, [
      el("mdui-icon-info", { slot: "icon" }), "状态",
    ]),
    el("mdui-navigation-bar-item", { value: "list" }, [
      el("mdui-icon-apps", { slot: "icon" }), "动画",
    ]),
    el("mdui-navigation-bar-item", { value: "install" }, [
      el("mdui-icon-add", { slot: "icon" }), "安装",
    ]),
  ]));
}

// ==================== 初始化 ====================
buildApp();
FilePicker.init();
refreshStatus();