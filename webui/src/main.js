import 'mdui/mdui.css';
import 'mdui';

import '@mdui/icons/info.js';
import '@mdui/icons/apps.js';
import '@mdui/icons/add.js';
import '@mdui/icons/folder.js';
import '@mdui/icons/folder-zip.js';
import '@mdui/icons/chevron-right.js';
import '@mdui/icons/check-circle.js';
import '@mdui/icons/error.js';
import '@mdui/icons/lock.js';
import '@mdui/icons/play-circle.js';
import '@mdui/icons/delete.js';

// ==================== 常量 ====================
const MODULE_ID = "bootanimation_manager";
const MODULE_DIR = `/data/adb/modules/${MODULE_ID}`;
const UTILS_CMD = `cd ${MODULE_DIR} && MODDIR=${MODULE_DIR} ./utils`;

const ksu = window.ksu || window.ksuwebui || {};

// ==================== 工具 ====================
function toast(msg) {
  if (ksu.toast) { ksu.toast(msg); return; }
  const el = document.createElement("div");
  el.textContent = msg;
  el.style.cssText = "position:fixed;bottom:90px;left:50%;transform:translateX(-50%);background:#333;color:#fff;padding:10px 20px;border-radius:24px;font-size:0.85rem;z-index:9999;box-shadow:0 4px 12px rgba(0,0,0,0.4);white-space:pre-wrap;max-width:80vw;text-align:center;";
  document.body.appendChild(el);
  setTimeout(() => el.remove(), 2600);
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

function escapeHtml(s) {
  return String(s).replace(/[&<>"]/g, c => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[c]));
}

function sleep(ms) {
  return new Promise(r => setTimeout(r, ms));
}

function shortHash(h) {
  return h ? h.slice(0, 16) : null;
}

// ==================== DOM 构建工具 ====================
function el(tag, attrs = {}, children = []) {
  const node = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (v === false || v === null || v === undefined) continue;
    if (k === "style") node.setAttribute("style", v);
    else if (k === "html") node.innerHTML = v;
    else if (k === "text") node.textContent = v;
    else if (k === "on") {
      for (const [ev, fn] of Object.entries(v)) node.addEventListener(ev, fn);
    } else node.setAttribute(k, v);
  }
  for (const c of [].concat(children)) {
    if (c === null || c === undefined || c === false) continue;
    node.appendChild(typeof c === "string" ? document.createTextNode(c) : c);
  }
  return node;
}

const C = {
  pageWrap: "padding:16px;max-width:720px;margin:0 auto;",
  pageTitle: "font-size:1.3rem;font-weight:600;margin-bottom:16px;",
  card: "width:100%;margin-bottom:16px;",
  cardPad: "padding:16px;",
  row: "display:flex;justify-content:space-between;align-items:center;padding:8px 0;font-size:0.85rem;gap:12px;",
  muted: "opacity:0.6;",
  mono: "font-family:monospace;font-size:0.8rem;",
  badgeOk: "background:#166534;color:#86efac;padding:2px 8px;border-radius:6px;font-size:0.7rem;font-weight:600;",
  badgeWarn: "background:#713f12;color:#fde047;padding:2px 8px;border-radius:6px;font-size:0.7rem;font-weight:600;",
  badgeGray: "background:#3f3f46;color:#d4d4d8;padding:2px 8px;border-radius:6px;font-size:0.7rem;font-weight:600;",
  metaBox: "margin-top:10px;padding:10px;background:var(--mdui-color-surface-variant);border-radius:8px;font-size:0.8rem;",
  sectionTitle: "font-weight:600;font-size:0.95rem;margin-bottom:12px;",
  centerMsg: "text-align:center;padding:24px;opacity:0.4;font-size:0.85rem;",
};

// ==================== 全局状态 ====================
const state = {
  status: null,      // 最近一次 status 结果
  list: [],          // 动画列表 [{id, name}]
  meta: {},          // id -> info 元数据
  pickPath: "/sdcard",
  pickSelected: null,
  checkResult: null, // 最近一次 check 结果
};

// ==================== 页面结构 ====================
const pages = {};

function buildApp() {
  const root = document.body;

  // ---------- 状态页 ----------
  const statusContent = el("div", { id: "status-content", html: '<div style="' + C.centerMsg + '">读取中...</div>' });
  const statusPage = el("div", { class: "page", id: "page-status" },
    el("div", { style: C.pageWrap }, [
      el("div", { style: C.pageTitle, text: "状态" }),
      el("mdui-card", { style: C.card },
        el("div", { style: C.cardPad, id: "status-info" }, statusContent)),
      el("mdui-card", { style: C.card },
        el("div", { style: C.cardPad }, [
          el("div", { style: C.sectionTitle, text: "操作" }),
          el("div", { style: "display:flex;gap:8px;flex-wrap:wrap;", id: "status-actions" },
            el("mdui-button", { variant: "filled", id: "btn-apply", text: "应用当前配置" }),
            el("mdui-button", { variant: "outlined", id: "btn-revert", text: "卸载动画" }),
            el("mdui-button", { variant: "text", id: "btn-check", text: "检查生效" }),
          )),
        ])),
    ]));

  // ---------- 动画列表页 ----------
  const listPage = el("div", { class: "page", id: "page-list" },
    el("div", { style: C.pageWrap }, [
      el("div", { style: C.pageTitle, text: "已安装动画" }),
      el("div", { id: "anim-list", html: '<div style="' + C.centerMsg + '">加载中...</div>' }),
    ]));

  // ---------- 安装页 ----------
  const installPath = el("div", { id: "install-path", style: "font-size:0.78rem;font-family:monospace;opacity:0.7;word-break:break-all;margin-bottom:8px;", text: "/sdcard" });
  const installBody = el("div", { id: "install-body", html: '<div style="' + C.centerMsg + '">加载中...</div>' });
  const btnConfirm = el("mdui-button", { variant: "filled", id: "btn-confirm", style: "flex:1;", disabled: "", text: "安装选中文件" });
  const installPage = el("div", { class: "page", id: "page-install" },
    el("div", { style: C.pageWrap }, [
      el("div", { style: C.pageTitle, text: "安装动画包" }),
      installPath,
      el("div", { style: "display:flex;gap:8px;margin-bottom:12px;" }, [
        el("mdui-button", { variant: "outlined", id: "btn-up", style: "flex:0 0 auto;", text: "上一级" }),
        btnConfirm,
      ]),
      installBody,
    ]));

  // ---------- 挂载 ----------
  pages.status = statusPage;
  pages.list = listPage;
  pages.install = installPage;

  root.appendChild(statusPage);
  root.appendChild(listPage);
  root.appendChild(installPage);

  const nav = el("mdui-navigation-bar", {
    value: "status", id: "bottom-nav", "label-visibility": "labeled",
    on: { change: (e) => switchPage(e.target.value) },
  }, [
    el("mdui-navigation-bar-item", { value: "status" }, [
      el("mdui-icon-info", { slot: "icon" }),
      "状态",
    ]),
    el("mdui-navigation-bar-item", { value: "list" }, [
      el("mdui-icon-apps", { slot: "icon" }),
      "动画",
    ]),
    el("mdui-navigation-bar-item", { value: "install" }, [
      el("mdui-icon-add", { slot: "icon" }),
      "安装",
    ]),
  ]);
  root.appendChild(nav);

  // ---------- 事件绑定 ----------
  document.getElementById("btn-apply").onclick = applyCurrent;
  document.getElementById("btn-revert").onclick = revert;
  document.getElementById("btn-check").onclick = check;
  document.getElementById("btn-up").onclick = () => FilePicker.goUp();
  btnConfirm.onclick = () => FilePicker.confirm();
}

// ==================== 页面切换 ====================
function switchPage(name) {
  for (const k in pages) pages[k].classList.remove("active");
  const target = pages[name];
  if (target) target.classList.add("active");
  if (name === "status") refreshStatus();
  if (name === "list") refreshList();
  if (name === "install") FilePicker.load(FilePicker.currentPath);
}

// ==================== 状态页 ====================
function mountModeBadge(nowId, cfgId) {
  // now 为空 → 未挂载/已卸载
  if (!nowId) return '<span style="' + C.badgeGray + '">未挂载</span>';
  if (!cfgId) return '<span style="' + C.badgeWarn + '">临时 / 一次性</span>';
  return nowId === cfgId
    ? '<span style="' + C.badgeOk + '">持久化</span>'
    : '<span style="' + C.badgeWarn + '">临时 / 一次性</span>';
}

async function refreshStatus() {
  const box = document.getElementById("status-info");
  box.innerHTML = '<div style="' + C.centerMsg + '">读取中...</div>';
  const { stdout } = await shAsync(`${UTILS_CMD} status --json`);
  const res = parseResult(stdout);
  if (!res || !res.success) {
    box.innerHTML = '<div style="' + C.centerMsg + '">无法读取状态</div>';
    return;
  }
  const d = res.data;
  state.status = d;

  const fmtText = d.system_format === "coloros" ? "ColorOS (g 格式)" : (d.system_format === "aosp" ? "AOSP (标准)" : (d.system_format || "-"));

  const rows = [
    ["系统路径", `<span style="${C.mono};text-align:right;word-break:break-all;">${escapeHtml(d.system_path || "(未检测到)")}</span>`],
    ["系统格式", fmtText],
    ["开机自动挂载", d.enabled
      ? '<span style="' + C.badgeOk + '">已启用</span>'
      : '<span style="' + C.badgeWarn + '">已禁用</span>'],
    ["界面语言", d.language ? escapeHtml(d.language) : '<span style="' + C.muted + '">自动检测</span>'],
    ["配置动画 ID", `<span style="${C.mono}">${escapeHtml(d.config_animation_id || "(未设置)")}</span>`],
    ["当前已挂载 ID", `<span style="${C.mono}">${escapeHtml(d.now_animation_id || "(未挂载)")}</span>`],
    ["挂载模式", mountModeBadge(d.now_animation_id, d.config_animation_id)],
    ["模块文件", d.module_file_exists ? "存在" : "缺失"],
    ["主动画挂载", d.main_mounted
      ? '<span style="' + C.badgeOk + '">已挂载</span>'
      : '<span style="' + C.badgeGray + '">未挂载</span>'],
  ];
  if (d.dark_mounted) rows.push(["Dark 动画", '<span style="' + C.badgeOk + '">已挂载</span>']);
  if (d.main_sources && d.main_sources.length) {
    rows.push(["挂载源", `<span style="${C.mono};font-size:0.72rem;text-align:right;word-break:break-all;">${escapeHtml(d.main_sources.join(", "))}</span>`]);
  }

  const content = el("div");
  for (const [k, v] of rows) {
    content.appendChild(el("div", { style: C.row }, [
      el("span", { style: C.muted, text: k }),
      el("span", { html: v, style: "text-align:right;" }),
    ]));
  }

  box.innerHTML = "";
  box.appendChild(content);
}

// ==================== 动画列表页 ====================
async function refreshList() {
  const box = document.getElementById("anim-list");
  box.innerHTML = '<div style="' + C.centerMsg + '">加载中...</div>';

  const [listRes, infoRes] = await Promise.all([
    shAsync(`${UTILS_CMD} list --json`),
    shAsync(`${UTILS_CMD} info --json`),
  ]);
  const list = parseResult(listRes.stdout);
  const info = parseResult(infoRes.stdout);

  if (!list || !list.success || !list.data || !list.data.animations || !list.data.animations.length) {
    box.innerHTML = '<div style="' + C.centerMsg + '">没有已安装的动画</div>';
    state.list = [];
    state.meta = {};
    return;
  }

  const metaMap = {};
  if (info && info.success && info.data && Array.isArray(info.data.info)) {
    for (const m of info.data.info) if (m && m.id) metaMap[m.id] = m;
  }
  state.list = list.data.animations;
  state.meta = metaMap;

  const st = state.status;
  const nowId = st ? st.now_animation_id : null;
  const cfgId = st ? st.config_animation_id : null;

  box.innerHTML = "";
  for (const item of list.data.animations) {
    const m = metaMap[item.id] || {};
    const isDefault = cfgId && cfgId === item.id;
    const isActive = nowId && nowId === item.id;

    const rows = [
      ["作者", m.author],
      ["版本", m.version ? `${m.version}${m.version_code ? ` (${m.version_code})` : ""}` : null],
      ["描述", m.desc],
      ["ID", item.id],
    ].filter(([, v]) => v !== undefined && v !== null && v !== "");

    const metaRows = rows.length
      ? rows.map(([k, v]) => el("div", { style: "display:flex;gap:8px;padding:3px 0;" }, [
          el("span", { style: C.muted + "min-width:64px;flex-shrink:0;", text: k }),
          el("span", { style: "word-break:break-word;", text: String(v) }),
        ]))
      : [el("div", { style: C.muted, text: "无更多元数据" })];

    const metaBox = el("div", { id: `meta-${item.id}`, style: C.metaBox, html: "" });
    metaRows.forEach(r => metaBox.appendChild(r));

    const tags = el("span", { style: "display:inline-flex;gap:4px;margin-left:6px;" });
    if (isDefault) tags.appendChild(el("span", { style: C.badgeOk, text: "默认" }));
    if (isActive && !isDefault) tags.appendChild(el("span", { style: C.badgeWarn, text: "当前挂载" }));
    else if (isActive && isDefault) tags.appendChild(el("span", { style: C.badgeGray, text: "运行中" }));

    const title = el("div", {
      style: "font-size:0.95rem;font-weight:600;cursor:pointer;display:flex;align-items:center;gap:4px;flex-wrap:wrap;",
      on: { click: () => {
        const open = metaBox.style.display === "none";
        metaBox.style.display = open ? "block" : "none";
        icon.style.transform = open ? "rotate(90deg)" : "";
      } },
    }, [
      icon = el("mdui-icon-chevron-right", { class: "chevron", style: "font-size:18px;transition:transform 0.15s;transform:rotate(90deg);" }),
      document.createTextNode(item.name || item.id),
      tags,
    ]);
    let icon; title.prepend(icon = el("mdui-icon-chevron-right", { class: "chevron", style: "font-size:18px;transition:transform 0.15s;transform:rotate(90deg);" }));

    const card = el("mdui-card", { style: C.card },
      el("div", { style: C.cardPad }, [
        el("div", { style: "display:flex;justify-content:space-between;align-items:flex-start;gap:8px;" }, [
          el("div", { style: "flex:1;min-width:0;" }, [
            title,
            el("div", { style: "font-size:0.72rem;" + C.muted + "font-family:monospace;margin-top:2px;", text: item.id }),
          ]),
          el("div", { style: "display:flex;gap:6px;flex-shrink:0;flex-wrap:wrap;" }, [
            el("mdui-button", { variant: "filled", style: "font-size:0.78rem;", text: "设为默认", on: { click: () => useAnimation(item.id) } }),
            el("mdui-button", { variant: "outlined", style: "font-size:0.78rem;", text: "临时挂载", on: { click: () => tempMount(item.id) } }),
            el("mdui-button", { variant: "outlined", style: "font-size:0.78rem;", text: "预览", on: { click: () => previewAnimation(item.id) } }),
            el("mdui-button", { variant: "text", style: "font-size:0.78rem;", text: "用一次", on: { click: () => onceApply(item.id) } }),
          ]),
        ]),
        metaBox,
      ]));
    box.appendChild(card);
  }
}

// ==================== 操作 ====================
async function runSimple(cmd, loadingMsg, successMsg) {
  toast(loadingMsg);
  const { stdout, stderr } = await shAsync(`${UTILS_CMD} ${cmd} --json`);
  const res = parseResult(stdout);
  if (res && res.success) toast(successMsg || (res.data && res.data.message) || "完成");
  else toast(res && res.error ? res.error : (stderr || "操作失败"));
  refreshStatus();
  return res;
}

async function useAnimation(id) {
  toast(`正在设置 ${id}...`);
  const { stdout, stderr } = await shAsync(`${UTILS_CMD} use ${shQuote(id)} --json`);
  const res = parseResult(stdout);
  if (res && res.success) { toast(`已设为默认：${id}`); }
  else toast(res && res.error ? res.error : (stderr || "设置失败"));
  refreshStatus();
  refreshList();
}

async function applyCurrent() {
  const res = await runSimple("apply", "正在应用...", null);
  if (res && res.success && res.data) toast(res.data.message || "应用成功");
  refreshList();
}

async function revert() {
  const res = await runSimple("revert", "正在卸载...", null);
  if (res && res.success && res.data) toast(res.data.message || "已卸载");
  refreshList();
}

async function check() {
  const btn = document.getElementById("btn-check");
  const old = btn.textContent;
  btn.disabled = true; btn.textContent = "检查中...";
  const { stdout } = await shAsync(`${UTILS_CMD} check --json`);
  btn.disabled = false; btn.textContent = old;
  const res = parseResult(stdout);
  state.checkResult = res && res.success ? res.data : null;
  if (!res || !res.success) { toast(res && res.error ? res.error : "检查失败"); return; }
  showCheckDialog(res.data);
}

async function tempMount(id) {
  toast(`临时挂载 ${id}（重启后失效）...`);
  const { stdout, stderr } = await shAsync(`${UTILS_CMD} apply --temp ${shQuote(id)} --json`);
  const res = parseResult(stdout);
  if (res && res.success) toast(res.data ? res.data.message : "临时挂载成功");
  else toast(res && res.error ? res.error : (stderr || "临时挂载失败"));
  refreshStatus();
  refreshList();
}

async function onceApply(id) {
  toast(`设置一次性动画：${id}（下次开机用一次）`);
  const { stdout, stderr } = await shAsync(`${UTILS_CMD} once_apply ${shQuote(id)} --json`);
  const res = parseResult(stdout);
  if (res && res.success) toast(res.data ? res.data.message : "已设置");
  else toast(res && res.error ? res.error : (stderr || "设置失败"));
  refreshStatus();
  refreshList();
}

async function toggleEnable(value) {
  const cmd = value ? "enable" : "disable";
  toast(value ? "正在启用..." : "正在禁用...");
  const { stdout, stderr } = await shAsync(`${UTILS_CMD} ${cmd} --json`);
  const res = parseResult(stdout);
  if (res && res.success) toast(res.data ? res.data.message : "完成");
  else toast(res && res.error ? res.error : (stderr || "操作失败"));
  refreshStatus();
}

// ==================== check 详情弹窗 ====================
function showCheckDialog(d) {
  const iconName = d.active ? "mdui-icon-check-circle" : "mdui-icon-error";
  const color = d.active ? "#16a34a" : "#dc2626";
  const titleText = d.active ? "动画已生效" : "动画未生效";

  const rows = [
    ["比对 ID", d.animation_id || "-"],
    ["now_bootanimation", d.now_animation_id || "(未设置)"],
    ["settings.bootanimation", d.config_animation_id || "(未设置)"],
    ["开机自动挂载", d.enabled ? "启用" : "禁用"],
    ["模块动画存在", d.module_exists ? "是" : "否"],
    ["系统动画存在", d.system_exists ? "是" : "否"],
    ["已挂载", d.mounted ? "是" : "否"],
    ["模块哈希", shortHash(d.module_hash) || "(不存在)"],
    ["系统哈希", shortHash(d.system_hash) || "(不存在)"],
    ["状态", d.active ? "已生效" : "未生效"],
  ];

  const list = el("div", { style: "font-size:0.85rem;" });
  for (const [k, v] of rows) {
    list.appendChild(el("div", { style: "display:flex;gap:10px;padding:5px 0;border-bottom:1px solid var(--mdui-color-outline-variant);" }, [
      el("span", { style: C.muted + "min-width:120px;flex-shrink:0;", text: k }),
      el("span", { style: "word-break:break-all;" + (k === "状态" ? `color:${color};font-weight:600;` : ""), text: String(v) }),
    ]));
  }

  const card = el("div", { style: "max-width:360px;width:100%;background:rgb(var(--mdui-color-surface-container-high));color:rgb(var(--mdui-color-on-surface));border-radius:16px;padding:20px;box-shadow:0 8px 32px rgba(0,0,0,0.5);" }, [
    el("div", { style: "display:flex;align-items:center;gap:10px;margin-bottom:4px;" }, [
      el(iconName, { style: `font-size:26px;color:${color};` }),
      el("div", { style: "font-size:1.1rem;font-weight:600;", text: titleText }),
    ]),
    el("div", { style: C.muted + "font-size:0.8rem;margin-bottom:14px;", text: d.message }),
    list,
    el("div", { style: "display:flex;justify-content:flex-end;margin-top:18px;" },
      el("mdui-button", { variant: "filled", id: "check-dialog-ok", text: "知道了" })),
  ]);

  const overlay = el("div", { style: "position:fixed;inset:0;background:rgba(0,0,0,0.6);z-index:9998;display:flex;align-items:center;justify-content:center;padding:24px;", on: { click: (e) => { if (e.target === overlay) overlay.remove(); } } }, card);
  document.body.appendChild(overlay);
  card.querySelector("#check-dialog-ok").onclick = () => overlay.remove();
}

// ==================== 预览功能 ====================
function showPreviewDialog(id) {
  return new Promise((resolve) => {
    const input = el("input", { id: "preview-seconds", type: "number", inputmode: "numeric", value: "0", min: "0", style: "width:100%;box-sizing:border-box;padding:10px 12px;border-radius:8px;border:1px solid rgb(var(--mdui-color-outline));background:rgb(var(--mdui-color-surface-container-lowest));color:rgb(var(--mdui-color-on-surface));font-size:0.95rem;font-family:monospace;outline:none;" });
    const dialog = el("div", { style: "max-width:320px;width:100%;background:rgb(var(--mdui-color-surface-container-high));color:rgb(var(--mdui-color-on-surface));border-radius:16px;padding:20px;box-shadow:0 8px 32px rgba(0,0,0,0.5);" }, [
      el("div", { style: "font-size:1.05rem;font-weight:600;margin-bottom:8px;", text: "预览动画" }),
      el("div", { style: C.muted + "font-size:0.8rem;margin-bottom:14px;word-break:break-all;font-family:monospace;", text: id }),
      el("div", { style: "font-size:0.82rem;opacity:0.75;margin-bottom:8px;", text: "输入预览秒数（0 = 无限，按音量键退出）" }),
      input,
      el("div", { style: "display:flex;justify-content:flex-end;gap:8px;margin-top:18px;" }, [
        el("mdui-button", { variant: "text", id: "preview-cancel", text: "取消" }),
        el("mdui-button", { variant: "filled", id: "preview-ok", text: "开始预览" }),
      ]),
    ]);

    const overlay = el("div", { style: "position:fixed;inset:0;background:rgba(0,0,0,0.6);z-index:9998;display:flex;align-items:center;justify-content:center;padding:24px;", on: { click: (e) => { if (e.target === overlay) { overlay.remove(); resolve(null); } } } }, dialog);
    document.body.appendChild(overlay);
    setTimeout(() => { input.focus(); input.select(); }, 50);

    const close = (r) => { overlay.remove(); resolve(r); };
    const confirm = () => {
      let num = parseInt(input.value.trim(), 10);
      if (isNaN(num) || num < 0) num = 0;
      close(num);
    };
    dialog.querySelector("#preview-cancel").onclick = () => close(null);
    dialog.querySelector("#preview-ok").onclick = confirm;
    input.onkeydown = (e) => {
      if (e.key === "Enter") { e.preventDefault(); confirm(); }
      if (e.key === "Escape") close(null);
    };
  });
}

async function previewAnimation(id) {
  const seconds = await showPreviewDialog(id);
  if (seconds === null) return;

  toast(`正在临时挂载 ${id}...`);
  const { stdout, stderr } = await shAsync(`${UTILS_CMD} apply --temp ${shQuote(id)} --json`);
  const res = parseResult(stdout);
  if (!res || !res.success) {
    toast(res && res.error ? res.error : (stderr || "临时挂载失败，已取消预览"));
    return;
  }

  toast("3 秒后开始预览...");
  await sleep(1000);
  toast("2...");
  await sleep(1000);
  toast("1...");
  await sleep(1000);

  toast(`正在预览（${seconds === 0 ? "无限，按音量键退出" : seconds + " 秒"}）`);
  shAsync(`cd ${shQuote(MODULE_DIR)} && MODDIR=${shQuote(MODULE_DIR)} sh action.sh ${shQuote(String(seconds))}`)
    .then(() => shAsync(`${UTILS_CMD} apply --json`))
    .then(() => refreshStatus());
}

// ==================== 安装页文件选择 ====================
const FilePicker = {
  currentPath: "/sdcard",
  selected: null,

  init() { this.load(this.currentPath); },

  updateConfirm() {
    const btn = document.getElementById("btn-confirm");
    btn.disabled = !this.selected;
    btn.textContent = this.selected ? `安装：${this.selected.split("/").pop()}` : "安装选中文件";
  },

  async load(path) {
    const body = document.getElementById("install-body");
    const pathEl = document.getElementById("install-path");
    body.innerHTML = '<div style="' + C.centerMsg + '">读取目录...</div>';
    pathEl.textContent = path;

    const { stdout, errno } = await shAsync(`LC_ALL=C ls -1pL ${shQuote(path)} 2>/dev/null`);
    if (errno !== 0 || !stdout.trim()) {
      body.innerHTML = '<div style="' + C.centerMsg + '">目录为空或无法读取</div>';
      this.currentPath = path;
      this.updateConfirm();
      return;
    }
    this.currentPath = path;

    const entries = stdout.split("\n").map(s => s.trim()).filter(Boolean);
    const dirs = entries.filter(n => n.endsWith("/"));
    const files = entries.filter(n => !n.endsWith("/"));
    const visible = arr => arr.filter(n => !n.startsWith("."));

    const frag = document.createDocumentFragment();
    for (const d of visible(dirs)) {
      const name = d.slice(0, -1);
      frag.appendChild(el("div", {
        class: "file-row", style: "display:flex;align-items:center;padding:12px 8px;cursor:pointer;border-bottom:1px solid var(--mdui-color-outline-variant);",
        on: { click: () => { this.selected = null; this.updateConfirm(); this.load(`${base()}/${name}`); } },
      }, [
        el("mdui-icon-folder", { style: "margin-right:10px;color:var(--mdui-color-primary);" }),
        el("span", { style: "font-size:0.88rem;flex:1;word-break:break-all;", text: name }),
      ]));
    }
    for (const f of visible(files)) {
      if (!f.toLowerCase().endsWith(".zip")) continue;
      const fname = f;
      frag.appendChild(el("div", {
        class: "file-row", style: "display:flex;align-items:center;padding:12px 8px;cursor:pointer;border-bottom:1px solid var(--mdui-color-outline-variant);",
        on: { click: () => { clearSel(); this.selected = `${base()}/${fname}`; this.updateConfirm(); markSel(fname); } },
      }, [
        el("mdui-icon-folder-zip", { style: "margin-right:10px;color:var(--mdui-color-tertiary);" }),
        el("span", { style: "font-size:0.88rem;flex:1;word-break:break-all;", text: f }),
        el("span", { style: "font-size:0.72rem;" + C.muted, text: "ZIP" }),
      ]));
    }

    body.innerHTML = "";
    if (!frag.childNodes.length) {
      body.innerHTML = '<div style="' + C.centerMsg + '">此目录下没有 zip 文件</div>';
    } else {
      body.appendChild(frag);
    }
    this.updateConfirm();

    function base() {
      const p = FilePicker.currentPath;
      return p.endsWith("/") ? p.slice(0, -1) : p;
    }
    function clearSel() {
      document.querySelectorAll("#install-body .file-row").forEach(r => { r.style.background = ""; });
    }
    function markSel(fname) {
      document.querySelectorAll("#install-body .file-row").forEach(r => {
        if (r.querySelector("span") && r.querySelector("span").textContent === fname) {
          r.style.background = "var(--mdui-color-secondary-container)";
        }
      });
    }
  },

  goUp() {
    const p = this.currentPath;
    if (p === "/" || p === "") return;
    const parent = p.replace(/\/+$/, "").split("/").slice(0, -1).join("/") || "/";
    this.load(parent);
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
      toast(res && res.error ? res.error : (stderr || "安装失败"));
    }

    this.selected = null;
    this.updateConfirm();
    refreshStatus();
    refreshList();
    this.load(this.currentPath);
  },
};

// ==================== 初始化 ====================
buildApp();
FilePicker.init();
switchPage("status");

