import { exec } from 'kernelsu';

// ===== 配置 =====
const MODULE_ID = 'bootanimation_manager';
const MODULE_DIR = `/data/adb/modules/${MODULE_ID}`;
const UTILS = `${MODULE_DIR}/utils`;
const LOGS_FILE = `${MODULE_DIR}/logs.txt`;
const HOME_PATH = '/sdcard';

// ===== 带超时的 exec =====
function execSafe(cmd, timeoutMs = 15000) {
    return Promise.race([
        exec(cmd),
        new Promise((_, reject) =>
            setTimeout(() => reject(new Error('命令超时: ' + cmd)), timeoutMs)
        )
    ]);
}

// ===== 核心：调用 utils 并解析 JSON =====
// 返回 { success, data, error, logs }
async function callUtils(args, timeoutMs = 30000) {
    const cmd = `"${UTILS}" --json --logs ${args} 2>"${LOGS_FILE}"`;
    let stdout = '';
    try {
        const r = await execSafe(cmd, timeoutMs);
        stdout = (r.stdout || '').trim();
    } catch (e) {
        return { success: false, data: null, error: e.message, logs: '' };
    }

    // 读取日志（可选展示）
    let logs = '';
    try {
        const lr = await execSafe(`cat "${LOGS_FILE}" 2>/dev/null`, 5000);
        logs = lr.stdout || '';
    } catch (_) {}

    if (!stdout) {
        return { success: false, data: null, error: 'utils 无输出（可能崩溃或参数错误）', logs };
    }

    try {
        const parsed = JSON.parse(stdout);
        return {
            success: !!parsed.success,
            data: parsed.data ?? null,
            error: parsed.error ?? null,
            logs
        };
    } catch (e) {
        return { success: false, data: null, error: 'JSON 解析失败: ' + stdout.slice(0, 200), logs };
    }
}

// ===== DOM =====
const statusBox = document.getElementById('statusBox');
const refreshStatusBtn = document.getElementById('refreshStatusBtn');
const animSelect = document.getElementById('animSelect');
const useBtn = document.getElementById('useBtn');
const applyBtn = document.getElementById('applyBtn');
const revertBtn = document.getElementById('revertBtn');
const actionStatus = document.getElementById('actionStatus');
const pathBar = document.getElementById('pathBar');
const fileListEl = document.getElementById('fileList');
const installStatusEl = document.getElementById('installStatus');
const logsBox = document.getElementById('logsBox');
const upBtn = document.getElementById('upBtn');
const homeBtn = document.getElementById('homeBtn');

function setText(el, msg, cls = '') {
    el.textContent = msg;
    el.className = 'status' + (cls ? ' ' + cls : '');
}

function showLogs(logs) {
    logsBox.textContent = logs || '（无日志）';
}

// ===== 状态刷新 =====
async function refreshStatus() {
    setText(statusBox, '正在获取状态...');
    const { success, data, error, logs } = await callUtils('status');
    showLogs(logs);

    if (!success) {
        setText(statusBox, '获取状态失败: ' + (error || '未知错误'), 'err');
        return;
    }

    const lines = [
        `系统路径: ${data.system_path ?? '-'}`,
        `当前动画: ${data.animation_id ?? '-'}`,
        `模块文件存在: ${data.module_file_exists ? '是' : '否'}`,
        `主动画已挂载: ${data.main_mounted ? '是' : '否'}`,
        `Dark 已挂载: ${data.dark_mounted ? '是' : '否'}`
    ];
    setText(statusBox, lines.join('\n'), data.main_mounted ? 'ok' : '');
}

// ===== 动画列表（用 ls 列目录，因为 utils 没有 list 命令） =====
async function loadAnimations() {
    animSelect.innerHTML = '<option value="">加载中...</option>';
    try {
        const r = await execSafe(`ls -p "${MODULE_DIR}/bootanimations" 2>/dev/null`);
        if (r.errno !== 0) {
            animSelect.innerHTML = '<option value="">读取目录失败</option>';
            return;
        }
        const entries = (r.stdout || '').trim().split('\n').map(s => s.trim()).filter(Boolean);
        const dirs = entries.filter(n => n.endsWith('/')).map(n => n.replace(/\/$/, ''));

        if (dirs.length === 0) {
            animSelect.innerHTML = '<option value="">没有可用动画</option>';
            return;
        }

        animSelect.innerHTML = '';
        for (const name of dirs) {
            const opt = document.createElement('option');
            opt.value = name;
            opt.textContent = name;
            animSelect.appendChild(opt);
        }
    } catch (e) {
        animSelect.innerHTML = '<option value="">加载失败</option>';
        setText(actionStatus, '加载动画列表出错: ' + e.message, 'err');
    }
}

// ===== 操作 =====
useBtn.onclick = async () => {
    const id = animSelect.value;
    if (!id) { setText(actionStatus, '请先选择一个动画', 'err'); return; }
    setText(actionStatus, '正在设为默认...');
    const { success, data, error, logs } = await callUtils(`use "${id}"`);
    showLogs(logs);
    if (success) {
        setText(actionStatus, `已设为默认: ${data.animation_id}`, 'ok');
        await refreshStatus();
    } else {
        setText(actionStatus, '设置失败: ' + (error || '未知错误'), 'err');
    }
};

applyBtn.onclick = async () => {
    setText(actionStatus, '正在应用...');
    const { success, data, error, logs } = await callUtils('apply');
    showLogs(logs);
    if (success) {
        setText(actionStatus, `已应用: ${data.animation_id ?? '-'}`, 'ok');
        await refreshStatus();
    } else {
        setText(actionStatus, '应用失败: ' + (error || '未知错误'), 'err');
    }
};

revertBtn.onclick = async () => {
    setText(actionStatus, '正在卸载...');
    const { success, data, error, logs } = await callUtils('revert');
    showLogs(logs);
    if (success) {
        setText(actionStatus, '已卸载', 'ok');
        await refreshStatus();
    } else {
        setText(actionStatus, '卸载失败: ' + (error || '未知错误'), 'err');
    }
};

refreshStatusBtn.onclick = refreshStatus;

// ===== 文件浏览器（保持系统 ls） =====
let currentPath = HOME_PATH;

async function listDir(path) {
    const r = await execSafe(`ls -p "${path}" 2>/dev/null`);
    if (r.errno !== 0) {
        setText(installStatusEl, `无法读取目录: ${path}`, 'err');
        return [];
    }
    return (r.stdout || '').trim().split('\n').map(s => s.trim()).filter(Boolean);
}

function goUp() {
    if (currentPath === '/') return;
    const parts = currentPath.split('/').filter(Boolean);
    parts.pop();
    currentPath = '/' + parts.join('/');
    refresh();
}

async function refresh() {
    pathBar.textContent = `当前路径: ${currentPath}`;
    fileListEl.innerHTML = '';
    setText(installStatusEl, '正在读取目录...');

    try {
        const entries = await listDir(currentPath);
        if (entries.length === 0) {
            const div = document.createElement('div');
            div.className = 'file-item disabled';
            div.textContent = '（空目录）';
            fileListEl.appendChild(div);
            setText(installStatusEl, '');
            return;
        }

        const dirs = entries.filter(n => n.endsWith('/'));
        const files = entries.filter(n => !n.endsWith('/'));

        for (const name of dirs) {
            const clean = name.replace(/\/$/, '');
            const div = document.createElement('div');
            div.className = 'file-item dir';
            div.textContent = '📁 ' + clean;
            div.onclick = () => {
                currentPath = (currentPath === '/' ? '' : currentPath) + '/' + clean;
                refresh();
            };
            fileListEl.appendChild(div);
        }

        for (const name of files) {
            const div = document.createElement('div');
            if (name.toLowerCase().endsWith('.zip')) {
                div.className = 'file-item zip';
                div.textContent = '📦 ' + name;
                div.onclick = () => installFromPath(currentPath + '/' + name);
            } else {
                div.className = 'file-item disabled';
                div.textContent = '📄 ' + name;
            }
            fileListEl.appendChild(div);
        }
        setText(installStatusEl, '');
    } catch (e) {
        setText(installStatusEl, '读取目录出错: ' + e.message, 'err');
    }
}

async function installFromPath(zipPath) {
    setText(installStatusEl, `正在安装: ${zipPath} ...`);
    const { success, data, error, logs } = await callUtils(`install "${zipPath}"`, 120000);
    showLogs(logs);

    if (success) {
        const installed = (data.installed || []).join(', ') || '无';
        const updated = (data.updated || []).join(', ') || '无';
        const skipped = (data.skipped || []).join(', ') || '无';
        setText(installStatusEl,
            `安装完成\n新增: ${installed}\n更新: ${updated}\n跳过: ${skipped}`,
            'ok');
        await loadAnimations();
        await refreshStatus();
    } else {
        setText(installStatusEl, '安装失败: ' + (error || '未知错误'), 'err');
    }
}

upBtn.onclick = goUp;
homeBtn.onclick = () => { currentPath = HOME_PATH; refresh(); };

// ===== 初始化：串行 =====
(async () => {
    try { await refreshStatus(); } catch (e) { console.error(e); }
    try { await loadAnimations(); } catch (e) { console.error(e); }
    try { await refresh(); } catch (e) { console.error(e); }
})();