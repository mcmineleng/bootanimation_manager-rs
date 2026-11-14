// ==================== 常量与工具 ====================
const MODULE_ID = "bootanimation_manager";
const MODULE_DIR = `/data/adb/modules/${MODULE_ID}`;
const UTILS = `${MODULE_DIR}/utils`;

const ksu = window.ksu || window.ksuwebui || {};

function toast(msg) {
  if (ksu.toast) { ksu.toast(msg); return; }
  const container = document.getElementById("toast-container");
  const el = document.createElement("div");
  el.className = "toast";
  el.textContent = msg;
  container.appendChild(el);
  setTimeout(() => el.remove(), 2400);
}

// 异步执行 shell 命令（兼容 ksu.exec 回调形式）
function shAsync(cmd) {
  return new Promise((resolve) => {
    if (!ksu.exec) {
      resolve({ errno: -1, stdout: "", stderr: "KSU API 不可用" });
      return;
    }
    const cb = `_cb_${Date.now()}_${Math.random().toString(36).slice(2)}`;
    window[cb] = (errno, stdout, stderr) => {
      delete window[cb];
      resolve({ errno, stdout: stdout || "", stderr: stderr || "" });
    };
    try { ksu.exec(cmd, cb); }
    catch (e) { delete window[cb]; resolve({ errno: -1, stdout: "", stderr: e.message }); }
  });
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

// ==================== 状态刷新 ====================
async function refreshStatus() {
  const el = document.getElementById("status-content");
  el.innerHTML = '<div class="loading">读取中...</div>';
  const { stdout } = await shAsync(`${UTILS} status --json`);
  const res = parseResult(stdout);
  if (!res || !res.success) { el.innerHTML = '<div class="empty-hint">无法读取状态</div>'; return; }
  const d = res.data;
  const mountedBadge = d.main_mounted
    ? '<span class="badge badge-ok">已挂载</span>'
    : '<span class="badge badge-warn">未挂载</span>';
  el.innerHTML = `
    <div class="status-row"><span class="status-label">系统路径</span><span class="status-value">${escapeHtml(d.system_path || "(未检测到)")}</span></div>
    <div class="status-row"><span class="status-label">配置动画 ID</span><span class="status-value">${escapeHtml(d.animation_id || "(未设置)")}</span></div>
    <div class="status-row"><span class="status-label">模块文件</span><span class="status-value">${d.module_file_exists ? "存在" : "缺失"}</span></div>
    <div class="status-row"><span class="status-label">挂载状态</span><span class="status-value">${mountedBadge}</span></div>
    ${d.dark_mounted ? '<div class="status-row"><span class="status-label">Dark 模式</span><span class="status-value"><span class="badge badge-ok">已挂载</span></span></div>' : ""}
  `;
}

// ==================== 动画列表（含元数据） ====================
async function refreshList() {
  const el = document.getElementById("anim-list");
  el.innerHTML = '<div class="loading">加载中...</div>';

  const [listRes, infoRes] = await Promise.all([
    shAsync(`${UTILS} list --json`),
    shAsync(`${UTILS} info --json`),
  ]);
  const list = parseResult(listRes.stdout);
  const info = parseResult(infoRes.stdout);

  if (!list || !list.success || !list.data || !list.data.animations || list.data.animations.length === 0) {
    el.innerHTML = '<div class="empty-hint">没有已安装的动画</div>';
    return;
  }

  // 用 info 结果建 id -> 完整元数据 映射
  const metaMap = {};
  if (info && info.success && info.data && Array.isArray(info.data.info)) {
    for (const m of info.data.info) {
      if (m && m.id) metaMap[m.id] = m;
    }
  }

  el.innerHTML = list.data.animations.map(item => {
    const m = metaMap[item.id] || {};
    const metaRows = [
      ["作者", m.author],
      ["版本", m.version ? `${m.version}${m.version_code ? ` (${m.version_code})` : ""}` : null],
      ["描述", m.desc],
      ["ID", item.id],
    ].filter(([, v]) => v !== undefined && v !== null && v !== "");

    return `
      <div class="anim-item">
        <div class="anim-head">
          <div style="flex:1;min-width:0;">
            <div class="anim-title" data-toggle="${escapeHtml(item.id)}">
              <span class="chevron">▶</span>${escapeHtml(item.name || item.id)}
            </div>
            <div class="anim-id">${escapeHtml(item.id)}</div>
          </div>
          <div class="anim-actions">
            <button class="btn btn-sm btn-primary" data-use="${escapeHtml(item.id)}">设为默认</button>
            <button class="btn btn-sm" data-temp="${escapeHtml(item.id)}">预览</button>
          </div>
        </div>
        <div class="anim-meta" id="meta-${escapeHtml(item.id)}">
          ${metaRows.length
            ? metaRows.map(([k, v]) => `<div class="meta-row"><span class="meta-label">${k}</span><span class="meta-value">${escapeHtml(String(v))}</span></div>`).join("")
            : '<div class="meta-row"><span class="meta-value" style="color:#666">无更多元数据</span></div>'}
        </div>
      </div>`;
  }).join("");

  // 展开/折叠
  el.querySelectorAll("[data-toggle]").forEach(t => {
    t.onclick = () => {
      const meta = document.getElementById(`meta-${t.dataset.toggle}`);
      if (!meta) return;
      const open = meta.classList.toggle("open");
      t.querySelector(".chevron").textContent = open ? "▼" : "▶";
    };
  });

  el.querySelectorAll("[data-use]").forEach(b => b.onclick = () => useAnimation(b.dataset.use));
  el.querySelectorAll("[data-temp]").forEach(b => b.onclick = () => applyTemp(b.dataset.temp));
}

// ==================== 操作函数 ====================
async function useAnimation(id) {
  toast(`正在设置 ${id}...`);
  const { stdout, stderr } = await shAsync(`${UTILS} use ${id} --json`);
  const res = parseResult(stdout);
  if (res && res.success) { toast(`已设为默认：${id}`); refreshStatus(); }
  else toast(res?.error || stderr || "设置失败");
}

async function applyTemp(id) {
  toast(`临时挂载 ${id}...`);
  const { stdout, stderr } = await shAsync(`${UTILS} apply --temp ${id} --json`);
  const res = parseResult(stdout);
  if (res && res.success) { toast("已临时挂载（重启失效）"); refreshStatus(); }
  else toast(res?.error || stderr || "挂载失败");
}

async function applyCurrent() {
  toast("正在应用...");
  const { stdout, stderr } = await shAsync(`${UTILS} apply --json`);
  const res = parseResult(stdout);
  if (res && res.success) toast(res.data?.message || "应用成功");
  else toast(res?.error || stderr || "应用失败");
  refreshStatus();
}

async function revert() {
  toast("正在卸载...");
  const { stdout, stderr } = await shAsync(`${UTILS} revert --json`);
  const res = parseResult(stdout);
  if (res && res.success) toast(res.data?.message || "已卸载");
  else toast(res?.error || stderr || "卸载失败");
  refreshStatus();
}

async function check() {
  const { stdout } = await shAsync(`${UTILS} check --json`);
  const res = parseResult(stdout);
  if (res && res.success) toast(res.data.message);
  else toast("检查失败");
}

// ==================== 自制文件选择器 ====================
const FilePicker = {
  currentPath: "/sdcard",
  selected: null,

  open() {
    document.getElementById("install-modal").classList.add("open");
    this.selected = null;
    this.updateConfirm();
    this.load(this.currentPath);
  },

  close() {
    document.getElementById("install-modal").classList.remove("open");
    this.selected = null;
    this.updateConfirm();
  },

  updateConfirm() {
    const btn = document.getElementById("modal-confirm");
    btn.disabled = !this.selected;
    btn.textContent = this.selected
      ? `安装：${this.selected.split("/").pop()}`
      : "安装选中文件";
  },

  async load(path) {
    const body = document.getElementById("modal-body");
    const pathEl = document.getElementById("modal-path");
    body.innerHTML = '<div class="loading">读取目录...</div>';
    pathEl.textContent = path;

    // ls -1pL 让目录后缀带 /，文件不带
    // 用 LC_ALL=C 保持稳定排序
    const { stdout, errno } = await shAsync(`LC_ALL=C ls -1pL ${JSON.stringify(path)} 2>/dev/null`);
    if (errno !== 0 || !stdout.trim()) {
      body.innerHTML = '<div class="empty-hint">目录为空或无法读取</div>';
      this.currentPath = path;
      return;
    }
    this.currentPath = path;

    const entries = stdout.split("\n").map(s => s.trim()).filter(Boolean);

    // 目录在前，文件在后
    const dirs = entries.filter(n => n.endsWith("/"));
    const files = entries.filter(n => !n.endsWith("/"));

    let html = "";

    // 隐藏目录不显示（以 . 开头）
    const visible = arr => arr.filter(n => !n.startsWith("."));

    for (const d of visible(dirs)) {
      const name = d.slice(0, -1);
      html += `
        <div class="file-row dir" data-dir="${escapeHtml(name)}">
          <span class="file-icon">📁</span>
          <span class="file-name">${escapeHtml(name)}/</span>
        </div>`;
    }
    for (const f of visible(files)) {
      if (!f.toLowerCase().endsWith(".zip")) continue;
      html += `
        <div class="file-row" data-file="${escapeHtml(f)}">
          <span class="file-icon">📦</span>
          <span class="file-name">${escapeHtml(f)}</span>
          <span class="file-hint">ZIP</span>
        </div>`;
    }

    if (!html) {
      html = '<div class="empty-hint">此目录下没有 zip 文件</div>';
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
        body.querySelectorAll(".file-row.selected").forEach(r => r.classList.remove("selected"));
        row.classList.add("selected");
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
    this.close();
    toast("正在安装，请稍候...");

    const { stdout, stderr } = await shAsync(`${UTILS} install ${JSON.stringify(zipPath)} --json`);
    const res = parseResult(stdout);

    if (res && res.success && res.data) {
      const { installed = [], updated = [], skipped = [] } = res.data;
      const parts = [];
      if (installed.length) parts.push(`安装 ${installed.length} 个`);
      if (updated.length) parts.push(`更新 ${updated.length} 个`);
      if (skipped.length) parts.push(`跳过 ${skipped.length} 个`);
      toast(parts.length ? `完成：${parts.join("，")}` : "安装完成");

      if (skipped.length) {
        setTimeout(() => toast(`跳过：${skipped.join(", ")}`), 800);
      }
    } else {
      toast(res?.error || stderr || "安装失败");
    }

    refreshStatus();
    refreshList();
  },
};

// ==================== 初始化 ====================
document.getElementById("btn-apply").onclick = applyCurrent;
document.getElementById("btn-revert").onclick = revert;
document.getElementById("btn-check").onclick = check;
document.getElementById("btn-install").onclick = () => FilePicker.open();
document.getElementById("modal-close").onclick = () => FilePicker.close();
document.getElementById("modal-up").onclick = () => FilePicker.goUp();
document.getElementById("modal-confirm").onclick = () => FilePicker.confirm();

// 点击遮罩关闭
document.getElementById("install-modal").onclick = (e) => {
  if (e.target.id === "install-modal") FilePicker.close();
};

refreshStatus();
refreshList();
