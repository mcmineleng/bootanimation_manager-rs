import 'mdui/mdui.css';
import 'mdui';

import '@mdui/icons/info.js';
import '@mdui/icons/apps.js';
import '@mdui/icons/add.js';
import '@mdui/icons/folder.js';
import '@mdui/icons/folder-zip.js';
import '@mdui/icons/chevron-right.js';

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
  el.style.cssText = "position:fixed;bottom:90px;left:50%;transform:translateX(-50%);background:#333;color:#fff;padding:10px 20px;border-radius:24px;font-size:0.85rem;z-index:9999;box-shadow:0 4px 12px rgba(0,0,0,0.4);";
  document.body.appendChild(el);
  setTimeout(() => el.remove(), 2400);
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

// ==================== 状态页 ====================
async function refreshStatus() {
  const el = document.getElementById("status-content");
  el.innerHTML = '<div style="text-align:center;padding:20px;opacity:0.5;">读取中...</div>';
  const { stdout } = await shAsync(`${UTILS_CMD} status --json`);
  const res = parseResult(stdout);
  if (!res || !res.success) {
    el.innerHTML = '<div style="text-align:center;padding:20px;opacity:0.5;">无法读取状态</div>';
    return;
  }
  const d = res.data;
  const badge = d.main_mounted
    ? '<span style="background:#166534;color:#86efac;padding:2px 8px;border-radius:6px;font-size:0.7rem;font-weight:600;">已挂载</span>'
    : '<span style="background:#713f12;color:#fde047;padding:2px 8px;border-radius:6px;font-size:0.7rem;font-weight:600;">未挂载</span>';

  el.innerHTML = `
    <div style="display:flex;justify-content:space-between;padding:6px 0;font-size:0.85rem;">
      <span style="opacity:0.6;">系统路径</span>
      <span style="font-family:monospace;font-size:0.8rem;text-align:right;max-width:60%;word-break:break-all;">${escapeHtml(d.system_path || "(未检测到)")}</span>
    </div>
    <div style="display:flex;justify-content:space-between;padding:6px 0;font-size:0.85rem;">
      <span style="opacity:0.6;">配置动画 ID</span>
      <span style="font-family:monospace;font-size:0.8rem;">${escapeHtml(d.animation_id || "(未设置)")}</span>
    </div>
    <div style="display:flex;justify-content:space-between;padding:6px 0;font-size:0.85rem;">
      <span style="opacity:0.6;">模块文件</span>
      <span>${d.module_file_exists ? "存在" : "缺失"}</span>
    </div>
    <div style="display:flex;justify-content:space-between;padding:6px 0;font-size:0.85rem;">
      <span style="opacity:0.6;">挂载状态</span>
      <span>${badge}</span>
    </div>
    ${d.dark_mounted ? `<div style="display:flex;justify-content:space-between;padding:6px 0;font-size:0.85rem;"><span style="opacity:0.6;">Dark 模式</span><span style="background:#166534;color:#86efac;padding:2px 8px;border-radius:6px;font-size:0.7rem;font-weight:600;">已挂载</span></div>` : ""}
  `;
}

// ==================== 动画列表页 ====================
async function refreshList() {
  const el = document.getElementById("anim-list");
  el.innerHTML = '<div style="text-align:center;padding:20px;opacity:0.5;">加载中...</div>';

  const [listRes, infoRes] = await Promise.all([
    shAsync(`${UTILS_CMD} list --json`),
    shAsync(`${UTILS_CMD} info --json`),
  ]);
  const list = parseResult(listRes.stdout);
  const info = parseResult(infoRes.stdout);

  if (!list || !list.success || !list.data || !list.data.animations || list.data.animations.length === 0) {
    el.innerHTML = '<div style="text-align:center;padding:24px;opacity:0.4;font-size:0.85rem;">没有已安装的动画</div>';
    return;
  }

  const metaMap = {};
  if (info && info.success && info.data && Array.isArray(info.data.info)) {
    for (const m of info.data.info) if (m && m.id) metaMap[m.id] = m;
  }

  el.innerHTML = list.data.animations.map(item => {
    const m = metaMap[item.id] || {};
    const rows = [
      ["作者", m.author],
      ["版本", m.version ? `${m.version}${m.version_code ? ` (${m.version_code})` : ""}` : null],
      ["描述", m.desc],
      ["ID", item.id],
    ].filter(([, v]) => v !== undefined && v !== null && v !== "");

    return `
      <mdui-card style="width:100%; margin-bottom:12px;">
        <div style="padding:14px;">
          <div style="display:flex;justify-content:space-between;align-items:flex-start;gap:8px;">
            <div style="flex:1;min-width:0;">
              <div style="font-size:0.95rem;font-weight:600;cursor:pointer;display:flex;align-items:center;gap:4px;" data-toggle="${escapeHtml(item.id)}">
                <mdui-icon-chevron-right class="chevron" style="font-size:18px;transition:transform 0.15s;transform:rotate(90deg);"></mdui-icon-chevron-right>
                ${escapeHtml(item.name || item.id)}
              </div>
              <div style="font-size:0.72rem;opacity:0.5;font-family:monospace;margin-top:2px;">${escapeHtml(item.id)}</div>
            </div>
            <div style="display:flex;gap:6px;flex-shrink:0;">
              <mdui-button variant="filled" data-use="${escapeHtml(item.id)}" style="font-size:0.78rem;">设为默认</mdui-button>
              <mdui-button variant="outlined" data-temp="${escapeHtml(item.id)}" style="font-size:0.78rem;">预览</mdui-button>
            </div>
          </div>
          <div id="meta-${escapeHtml(item.id)}" style="margin-top:10px;padding:10px;background:var(--mdui-color-surface-variant);border-radius:8px;font-size:0.8rem;">
            ${rows.length
              ? rows.map(([k, v]) => `<div style="display:flex;gap:8px;padding:3px 0;"><span style="opacity:0.6;min-width:64px;flex-shrink:0;">${k}</span><span style="word-break:break-word;">${escapeHtml(String(v))}</span></div>`).join("")
              : '<div style="opacity:0.4;">无更多元数据</div>'}
          </div>
        </div>
      </mdui-card>`;
  }).join("");

  el.querySelectorAll("[data-toggle]").forEach(t => {
    t.onclick = () => {
      const meta = document.getElementById(`meta-${t.dataset.toggle}`);
      if (!meta) return;
      const open = meta.style.display === "none";
      meta.style.display = open ? "block" : "none";
      const icon = t.querySelector(".chevron");
      if (icon) icon.style.transform = open ? "rotate(90deg)" : "";
    };
  });

  el.querySelectorAll("[data-use]").forEach(b => b.onclick = () => useAnimation(b.dataset.use));
  el.querySelectorAll("[data-temp]").forEach(b => b.onclick = () => previewAnimation(b.dataset.temp));
}

// ==================== 操作 ====================
async function useAnimation(id) {
  toast(`正在设置 ${id}...`);
  const { stdout, stderr } = await shAsync(`${UTILS_CMD} use ${shQuote(id)} --json`);
  const res = parseResult(stdout);
  if (res && res.success) { toast(`已设为默认：${id}`); refreshStatus(); }
  else toast(res?.error || stderr || "设置失败");
}

async function applyCurrent() {
  toast("正在应用...");
  const { stdout, stderr } = await shAsync(`${UTILS_CMD} apply --json`);
  const res = parseResult(stdout);
  if (res && res.success) toast(res.data?.message || "应用成功");
  else toast(res?.error || stderr || "应用失败");
  refreshStatus();
}

async function revert() {
  toast("正在卸载...");
  const { stdout, stderr } = await shAsync(`${UTILS_CMD} revert --json`);
  const res = parseResult(stdout);
  if (res && res.success) toast(res.data?.message || "已卸载");
  else toast(res?.error || stderr || "卸载失败");
  refreshStatus();
}

async function check() {
  const { stdout } = await shAsync(`${UTILS_CMD} check --json`);
  const res = parseResult(stdout);
  if (res && res.success) toast(res.data.message);
  else toast("检查失败");
}

// ==================== 预览功能 ====================
function showPreviewDialog(id) {
  return new Promise((resolve) => {
    const overlay = document.createElement("div");
    overlay.style.cssText = "position:fixed;inset:0;background:rgba(0,0,0,0.6);z-index:9998;display:flex;align-items:center;justify-content:center;padding:24px;";

    const dialog = document.createElement("div");
    dialog.style.cssText = "background:rgb(var(--mdui-color-surface-container-high, 236, 233, 230));color:rgb(var(--mdui-color-on-surface, 0, 0, 0));border-radius:16px;padding:20px;width:100%;max-width:320px;box-shadow:0 8px 32px rgba(0,0,0,0.5);";

    dialog.innerHTML = `
      <div style="font-size:1.05rem;font-weight:600;margin-bottom:8px;">预览动画</div>
      <div style="font-size:0.8rem;opacity:0.6;margin-bottom:14px;word-break:break-all;font-family:monospace;">${escapeHtml(id)}</div>
      <div style="font-size:0.82rem;opacity:0.75;margin-bottom:8px;">输入预览秒数（0 = 无限，按音量键退出）</div>
      <input id="preview-seconds" type="number" inputmode="numeric" value="0" min="0"
        style="width:100%;box-sizing:border-box;padding:10px 12px;border-radius:8px;border:1px solid rgb(var(--mdui-color-outline, 121, 116, 126));background:rgb(var(--mdui-color-surface-container-lowest, 255, 255, 255));color:rgb(var(--mdui-color-on-surface, 0, 0, 0));font-size:0.95rem;font-family:monospace;outline:none;" />
      <div style="display:flex;justify-content:flex-end;gap:8px;margin-top:18px;">
        <mdui-button variant="text" id="preview-cancel">取消</mdui-button>
        <mdui-button variant="filled" id="preview-ok">开始预览</mdui-button>
      </div>
    `;

    overlay.appendChild(dialog);
    document.body.appendChild(overlay);

    const input = dialog.querySelector("#preview-seconds");
    setTimeout(() => { input.focus(); input.select(); }, 50);

    const close = (result) => {
      overlay.remove();
      resolve(result);
    };

    dialog.querySelector("#preview-cancel").onclick = () => close(null);
    overlay.onclick = (e) => { if (e.target === overlay) close(null); };

    const confirm = () => {
      const raw = input.value.trim();
      // 空值或非数字统一按 0（无限）处理
      let num = parseInt(raw, 10);
      if (isNaN(num) || num < 0) num = 0;
      close(num);
    };

    dialog.querySelector("#preview-ok").onclick = confirm;
    input.onkeydown = (e) => {
      if (e.key === "Enter") { e.preventDefault(); confirm(); }
      if (e.key === "Escape") close(null);
    };
  });
}

async function previewAnimation(id) {
  const seconds = await showPreviewDialog(id);
  if (seconds === null) return; // 用户取消

  // 1. 先临时挂载指定动画（否则 action.sh 播放的是当前默认动画）
  toast(`正在挂载 ${id}...`);
  const { stdout, stderr } = await shAsync(`${UTILS_CMD} apply --temp ${shQuote(id)} --json`);
  const res = parseResult(stdout);
  if (!res || !res.success) {
    toast(res?.error || stderr || "临时挂载失败，已取消预览");
    return;
  }

  // 2. 倒计时 3 秒
  toast("3 秒后开始预览...");
  await sleep(1000);
  toast("2...");
  await sleep(1000);
  toast("1...");
  await sleep(1000);

  // 3. 执行 action.sh（不 await —— 无限预览需按音量键退出，可能很久才返回）
  toast(`正在预览 ${id}（${seconds === 0 ? "无限，按音量键退出" : seconds + " 秒"}）`);
  shAsync(`cd ${shQuote(MODULE_DIR)} && MODDIR=${shQuote(MODULE_DIR)} sh action.sh ${shQuote(String(seconds))}`)
    .then(() => {
      // 预览退出后恢复用户配置的默认动画
      return shAsync(`${UTILS_CMD} apply --json`);
    })
    .then(() => refreshStatus());
}

// ==================== 安装页文件选择 ====================
const FilePicker = {
  currentPath: "/sdcard",
  selected: null,

  init() {
    document.getElementById("btn-up").onclick = () => this.goUp();
    document.getElementById("btn-confirm").onclick = () => this.confirm();
    this.load(this.currentPath);
  },

  updateConfirm() {
    const btn = document.getElementById("btn-confirm");
    btn.disabled = !this.selected;
    btn.textContent = this.selected
      ? `安装：${this.selected.split("/").pop()}`
      : "安装选中文件";
  },

  async load(path) {
    const body = document.getElementById("install-body");
    const pathEl = document.getElementById("install-path");
    body.innerHTML = '<div style="text-align:center;padding:20px;opacity:0.5;">读取目录...</div>';
    pathEl.textContent = path;

    const { stdout, errno } = await shAsync(`LC_ALL=C ls -1pL ${shQuote(path)} 2>/dev/null`);
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

    let html = "";
    for (const d of visible(dirs)) {
      const name = d.slice(0, -1);
      html += `
        <div class="file-row" data-dir="${escapeHtml(name)}" style="display:flex;align-items:center;padding:12px 8px;cursor:pointer;border-bottom:1px solid var(--mdui-color-outline-variant);">
          <mdui-icon-folder style="margin-right:10px;color:var(--mdui-color-primary);"></mdui-icon-folder>
          <span style="font-size:0.88rem;flex:1;word-break:break-all;">${escapeHtml(name)}</span>
        </div>`;
    }
    for (const f of visible(files)) {
      if (!f.toLowerCase().endsWith(".zip")) continue;
      html += `
        <div class="file-row" data-file="${escapeHtml(f)}" style="display:flex;align-items:center;padding:12px 8px;cursor:pointer;border-bottom:1px solid var(--mdui-color-outline-variant);">
          <mdui-icon-folder-zip style="margin-right:10px;color:var(--mdui-color-tertiary);"></mdui-icon-folder-zip>
          <span style="font-size:0.88rem;flex:1;word-break:break-all;">${escapeHtml(f)}</span>
          <span style="font-size:0.72rem;opacity:0.5;">ZIP</span>
        </div>`;
    }

    if (!html) {
      html = '<div style="text-align:center;padding:20px;opacity:0.5;">此目录下没有 zip 文件</div>';
    }
    body.innerHTML = html;

    body.querySelectorAll("[data-dir]").forEach(row => {
      row.onclick = () => {
        const base = this.currentPath.endsWith("/") ? this.currentPath.slice(0, -1) : this.currentPath;
        this.selected = null;
        this.updateConfirm();
        this.load(`${base}/${row.dataset.dir}`);
      };
    });

    body.querySelectorAll("[data-file]").forEach(row => {
      row.onclick = () => {
        body.querySelectorAll(".file-row").forEach(r => { r.style.background = ""; });
        row.style.background = "var(--mdui-color-secondary-container)";
        const base = this.currentPath.endsWith("/") ? this.currentPath.slice(0, -1) : this.currentPath;
        this.selected = `${base}/${row.dataset.file}`;
        this.updateConfirm();
      };
    });
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
      toast(res?.error || stderr || "安装失败");
    }

    this.selected = null;
    this.updateConfirm();
    refreshStatus();
    refreshList();
    this.load(this.currentPath);
  },
};

// ==================== 页面切换 ====================
function switchPage(name) {
  document.querySelectorAll(".page").forEach(p => p.classList.remove("active"));
  document.getElementById(`page-${name}`).classList.add("active");
  if (name === "status") refreshStatus();
  if (name === "list") refreshList();
  if (name === "install") FilePicker.load(FilePicker.currentPath);
}

// ==================== 初始化 ====================
document.getElementById("btn-apply").onclick = applyCurrent;
document.getElementById("btn-revert").onclick = revert;
document.getElementById("btn-check").onclick = check;

document.getElementById("bottom-nav").addEventListener("change", (e) => {
  switchPage(e.target.value);
});

FilePicker.init();
refreshStatus();