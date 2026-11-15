use std::env;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::exit;
use std::sync::OnceLock;

use hex::encode as hex_encode;
use rustix::mount::{mount, unmount, MountFlags, UnmountFlags};
use sha2::{Digest, Sha256};
use zip as zip_crate;

use serde::{Deserialize, Serialize};

// ============================================================
// 常量
// ============================================================
const SETTINGS_JSON: &str = "settings.json";     // 持久化配置
const ONCE_TXT: &str = "once_bootanimation";     // 一次性：下次 apply 用完即删
const NOW_TXT: &str = "now_bootanimation";       // 当前实际应用的动画 ID

const ANIM_DIR: &str = "bootanimations";

const SYSTEM_CANDIDATES: &[&str] = &[
    "/system/product/media/bootanimation.zip",
    "/system/media/bootanimation.zip",
    "/product/media/bootanimation.zip",
    "/vendor/media/bootanimation.zip",
];

const COLOROS_CANDIDATES: &[&str] = &[
    "/my_product/media/bootanimation/bootanimation.zip",
    "/oppo_product/media/bootanimation/bootanimation.zip",
];

// ============================================================
// settings.json
// ============================================================
#[derive(Serialize, Deserialize, Clone)]
struct Settings {
    /// 默认动画 ID；None 表示未设置
    #[serde(default)]
    bootanimation: Option<String>,
    /// false 时 apply（开机自动挂载）直接失败；once_apply / apply --temp 不受影响
    #[serde(default = "default_enable")]
    enable: bool,
    /// 界面语言，最高优先级；"zh_CN" / "en_US" 等
    #[serde(default, skip_serializing_if = "Option::is_none")]
    language: Option<String>,
}

fn default_enable() -> bool {
    true
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            bootanimation: None,
            enable: true,
            language: None,
        }
    }
}

fn read_settings(moddir: &Path) -> Result<Settings, String> {
    let path = moddir.join(SETTINGS_JSON);
    if !path.exists() {
        return Ok(Settings::default());
    }
    let content = fs::read_to_string(&path)
        .map_err(|e| format!("读取 {} 失败: {}", path.display(), e))?;
    if content.trim().is_empty() {
        return Ok(Settings::default());
    }
    serde_json::from_str(&content)
        .map_err(|e| format!("解析 {} 失败: {}", path.display(), e))
}

fn write_settings(moddir: &Path, settings: &Settings) -> Result<(), String> {
    let path = moddir.join(SETTINGS_JSON);
    let content = serde_json::to_string_pretty(settings)
        .map_err(|e| format!("序列化 settings 失败: {}", e))?;
    fs::write(&path, content)
        .map_err(|e| format!("写入 {} 失败: {}", path.display(), e))
}

// ============================================================
// 语言检测
//
// 优先级：
//   1. settings.json 的 language 字段（zh* → Zh，en* → En）
//   2. INFO_ENG / INFO_EN 环境变量
//   3. LANG / LC_ALL / LC_MESSAGES / LANGUAGE
//   4. 默认 En
//
// 用 OnceLock 缓存，避免每次 log 都读文件；
// 直接读文件而不经过 read_settings，避免解析失败时 log 递归。
// ============================================================
#[derive(Clone, Copy, PartialEq, Debug)]
enum Lang {
    Zh,
    En,
}

fn detect_lang_from_settings() -> Option<Lang> {
    let moddir = mod_dir();
    let path = moddir.join(SETTINGS_JSON);
    if !path.exists() {
        return None;
    }
    let content = fs::read_to_string(&path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&content).ok()?;
    let lang = v.get("language")?.as_str()?.trim();
    if lang.is_empty() {
        return None;
    }
    let l = lang.to_lowercase();
    if l.starts_with("zh") {
        Some(Lang::Zh)
    } else if l.starts_with("en") {
        Some(Lang::En)
    } else {
        None
    }
}

fn detect_lang_from_env() -> Lang {
    for var in &["INFO_ENG", "INFO_EN"] {
        if let Ok(val) = env::var(var) {
            let v = val.to_lowercase();
            if v == "1" || v == "true" || v == "yes" || v == "y" || v == "on" {
                return Lang::En;
            }
        }
    }

    let check_var = |var: &str| -> Option<Lang> {
        let val = env::var(var).ok()?.to_lowercase();
        if val.starts_with("zh_cn")
            || val.starts_with("zh-cn")
            || val.starts_with("zh.cn")
        {
            Some(Lang::Zh)
        } else if val.starts_with("zh") {
            Some(Lang::En)
        } else {
            None
        }
    };

    check_var("LANG")
        .or_else(|| check_var("LC_ALL"))
        .or_else(|| check_var("LC_MESSAGES"))
        .or_else(|| check_var("LANGUAGE"))
        .unwrap_or(Lang::En)
}

fn detect_lang() -> Lang {
    static CACHE: OnceLock<Lang> = OnceLock::new();
    *CACHE.get_or_init(|| {
        detect_lang_from_settings().unwrap_or_else(detect_lang_from_env)
    })
}

// ============================================================
// 全局标志
// ============================================================
#[derive(Default)]
struct GlobalFlags {
    json: bool,
    logs: bool,
}

// ============================================================
// 日志
// ============================================================
#[cfg(unix)]
fn is_stderr_tty() -> bool {
    unsafe { libc::isatty(libc::STDERR_FILENO) != 0 }
}

#[cfg(not(unix))]
fn is_stderr_tty() -> bool {
    false
}

const C_RESET: &str = "\x1b[0m";
const C_GREEN: &str = "\x1b[32m";
const C_RED: &str = "\x1b[31m";
const C_YELLOW: &str = "\x1b[33m";
const C_CYAN: &str = "\x1b[36m";

fn colored(level: &str, color: &str) -> String {
    if is_stderr_tty() {
        format!("{}{}{}", color, level, C_RESET)
    } else {
        level.to_string()
    }
}

fn should_log(flags: &GlobalFlags) -> bool {
    if flags.json && !flags.logs {
        false
    } else {
        true
    }
}

fn log_info(flags: &GlobalFlags, msg: &str) {
    if !should_log(flags) {
        return;
    }
    let label = match detect_lang() {
        Lang::Zh => "[信息]",
        Lang::En => "[INFO]",
    };
    let prefix = colored(label, C_CYAN);
    let _ = writeln!(std::io::stderr(), "{} {}", prefix, msg);
}

fn log_ok(flags: &GlobalFlags, msg: &str) {
    if !should_log(flags) {
        return;
    }
    let label = match detect_lang() {
        Lang::Zh => "[成功]",
        Lang::En => "[OK]",
    };
    let prefix = colored(label, C_GREEN);
    let _ = writeln!(std::io::stderr(), "{} {}", prefix, msg);
}

fn log_warn(flags: &GlobalFlags, msg: &str) {
    if !should_log(flags) {
        return;
    }
    let label = match detect_lang() {
        Lang::Zh => "[警告]",
        Lang::En => "[WARN]",
    };
    let prefix = colored(label, C_YELLOW);
    let _ = writeln!(std::io::stderr(), "{} {}", prefix, msg);
}

fn log_error(flags: &GlobalFlags, msg: &str) {
    if !should_log(flags) {
        return;
    }
    let label = match detect_lang() {
        Lang::Zh => "[错误]",
        Lang::En => "[ERROR]",
    };
    let prefix = colored(label, C_RED);
    let _ = writeln!(std::io::stderr(), "{} {}", prefix, msg);
}

// ============================================================
// JSON 输出
// ============================================================
fn json_output<T: Serialize>(value: &T) -> Result<(), String> {
    let json = serde_json::to_string_pretty(value)
        .map_err(|e| format!("JSON 序列化失败: {}", e))?;
    println!("{}", json);
    Ok(())
}

// ============================================================
// 路径 / ID 读取工具
// ============================================================
fn mod_dir() -> PathBuf {
    if let Ok(dir) = env::var("MODDIR") {
        if !dir.is_empty() {
            return PathBuf::from(dir);
        }
    }
    env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

/// 从任意 ID 文件中读第一个有效行（跳过空行和 # 注释）
fn read_id_file(path: &Path) -> Result<Option<String>, String> {
    let file = match File::open(path) {
        Ok(f) => f,
        Err(_) => return Ok(None),
    };
    for line in BufReader::new(file).lines().flatten() {
        let s = line.trim();
        if !s.is_empty() && !s.starts_with('#') {
            return Ok(Some(s.to_string()));
        }
    }
    Ok(None)
}

/// 一次性动画（下次 apply 用完即删）
fn read_once_id(moddir: &Path) -> Result<Option<String>, String> {
    read_id_file(&moddir.join(ONCE_TXT))
}

/// 当前实际应用的动画
fn read_now_id(moddir: &Path) -> Result<Option<String>, String> {
    read_id_file(&moddir.join(NOW_TXT))
}

fn animation_zip(moddir: &Path, id: &str) -> PathBuf {
    moddir.join(ANIM_DIR).join(id).join("bootanimation.zip")
}

/// 返回 (系统动画路径, 是否为 ColorOS 路径)
fn detect_system_animation_ex() -> Option<(String, bool)> {
    for p in SYSTEM_CANDIDATES {
        if Path::new(p).exists() {
            return Some((p.to_string(), false));
        }
    }
    for p in COLOROS_CANDIDATES {
        if Path::new(p).exists() {
            return Some((p.to_string(), true));
        }
    }
    None
}

fn detect_system_animation() -> Option<String> {
    detect_system_animation_ex().map(|(p, _)| p)
}

fn dark_animation_path(sys: &str) -> Option<String> {
    let p = Path::new(sys).parent()?.join("bootanimation-dark.zip");
    p.exists().then(|| p.to_string_lossy().into_owned())
}

// ============================================================
// bind mount 封装
// ============================================================
fn bind_mount(src: &Path, dst: &Path) -> Result<(), String> {
    mount(src, dst, "", MountFlags::BIND, "")
        .map_err(|e| format!("mount 失败 {} -> {}: {:?}", src.display(), dst.display(), e))
}

fn bind_unmount(dst: &Path) -> Result<(), String> {
    unmount(dst, UnmountFlags::empty())
        .map_err(|e| format!("unmount 失败 {}: {:?}", dst.display(), e))
}

/// 强制卸载目标路径上的所有挂载层
fn force_unmount_all(target: &str) {
    let p = Path::new(target);
    for _ in 0..8 {
        if !is_mounted(target).unwrap_or(false) {
            return;
        }
        if unmount(p, UnmountFlags::empty()).is_err() {
            if unmount(p, UnmountFlags::MNT_DETACH).is_err() {
                return;
            }
        }
    }
}

// ============================================================
// 动画枚举 / 校验
// ============================================================
fn scan_animations(moddir: &Path) -> Vec<(String, serde_json::Value)> {
    let dir = moddir.join(ANIM_DIR);
    let mut out = Vec::new();
    let rd = match fs::read_dir(&dir) {
        Ok(r) => r,
        Err(_) => return out,
    };
    for e in rd.flatten() {
        let p = e.path();
        if !p.is_dir() {
            continue;
        }
        let id = p.file_name().unwrap().to_string_lossy().into_owned();
        let info_path = p.join("info.json");
        let val = match fs::read_to_string(&info_path)
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        {
            Some(v) => v,
            None => continue,
        };
        out.push((id, val));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

fn is_valid_animation(moddir: &Path, id: &str) -> bool {
    let base = moddir.join(ANIM_DIR).join(id);
    if !base.is_dir() {
        return false;
    }
    if !base.join("bootanimation.zip").is_file() {
        return false;
    }
    fs::read_to_string(base.join("info.json"))
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| {
            v.get("id")
                .and_then(|x| x.as_str())
                .map(|s| s == id)
        })
        .unwrap_or(false)
}

// ============================================================
// 哈希
// ============================================================
fn file_sha256(path: &str) -> Result<Option<String>, String> {
    if !Path::new(path).exists() {
        return Ok(None);
    }
    let data = fs::read(path).map_err(|e| format!("读取失败 {}: {}", path, e))?;
    let mut hasher = Sha256::new();
    hasher.update(&data);
    Ok(Some(hex_encode(hasher.finalize())))
}

// ============================================================
// /proc/mounts
// ============================================================
fn parse_proc_mounts() -> Result<Vec<(String, String)>, String> {
    let content = fs::read_to_string("/proc/mounts")
        .map_err(|_| "无法读取 /proc/mounts".to_string())?;
    let mut mounts = Vec::new();
    for line in content.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 {
            mounts.push((parts[0].to_string(), parts[1].to_string()));
        }
    }
    Ok(mounts)
}

fn is_mounted(target: &str) -> Result<bool, String> {
    let target = if let Ok(c) = fs::canonicalize(target) {
        c
    } else {
        return Ok(false);
    };
    let target = target.to_string_lossy().into_owned();
    Ok(parse_proc_mounts()?.iter().any(|(_, m)| m == &target))
}

fn mounted_sources(target: &str) -> Result<Vec<String>, String> {
    let target = if let Ok(c) = fs::canonicalize(target) {
        c
    } else {
        return Ok(vec![]);
    };
    let target = target.to_string_lossy().into_owned();
    Ok(parse_proc_mounts()?
        .iter()
        .filter_map(|(s, m)| if m == &target { Some(s.clone()) } else { None })
        .collect())
}

// ============================================================
// JSON 结构
// ============================================================
#[derive(Serialize)]
struct CmdResult<T: Serialize> {
    success: bool,
    data: Option<T>,
    error: Option<String>,
}

#[derive(Serialize)]
struct ApplyData {
    animation_id: Option<String>,
    message: String,
}

#[derive(Serialize)]
struct RevertData {
    unmounted: Vec<String>,
    message: String,
}

#[derive(Serialize)]
struct StatusData {
    system_path: Option<String>,
    system_format: Option<String>,
    enabled: bool,
    language: Option<String>,
    animation_id: Option<String>,
    now_animation_id: Option<String>,
    config_animation_id: Option<String>,
    module_file_exists: bool,
    main_mounted: bool,
    dark_mounted: bool,
    main_sources: Vec<String>,
}

#[derive(Serialize)]
struct CheckData {
    animation_id: Option<String>,
    now_animation_id: Option<String>,
    config_animation_id: Option<String>,
    enabled: bool,
    module_hash: Option<String>,
    system_hash: Option<String>,
    module_exists: bool,
    system_exists: bool,
    mounted: bool,
    active: bool,
    message: String,
}

#[derive(Serialize)]
struct UseData {
    animation_id: String,
    message: String,
}

#[derive(Serialize)]
struct ToggleData {
    enabled: bool,
    message: String,
}

#[derive(Serialize)]
struct ApplyTempData {
    animation_id: String,
    message: String,
    main_mounted: bool,
    dark_mounted: bool,
}

#[derive(Serialize)]
struct InstallData {
    system_format: String,
    installed: Vec<String>,
    updated: Vec<String>,
    skipped: Vec<String>,
    message: String,
}

#[derive(Serialize)]
struct ListItem {
    id: String,
    name: String,
}

#[derive(Serialize)]
struct ListData {
    animations: Vec<ListItem>,
}

#[derive(Serialize)]
struct InfoData {
    info: Vec<serde_json::Value>,
}

// ============================================================
// list
// ============================================================
fn list_cmd(flags: &GlobalFlags) -> Result<(), String> {
    let moddir = mod_dir();
    let anims = scan_animations(&moddir);

    let items: Vec<ListItem> = anims
        .iter()
        .map(|(id, v)| ListItem {
            id: id.clone(),
            name: v
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or(id)
                .to_string(),
        })
        .collect();

    if flags.json {
        json_output(&CmdResult {
            success: true,
            data: Some(ListData {
                animations: items,
            }),
            error: None,
        })?;
    } else if items.is_empty() {
        log_info(
            flags,
            match detect_lang() {
                Lang::Zh => "没有已安装的动画",
                Lang::En => "no animations installed",
            },
        );
    } else {
        for it in &items {
            eprintln!("{}\t{}", it.id, it.name);
        }
    }
    Ok(())
}

// ============================================================
// info
// ============================================================
fn info_cmd(flags: &GlobalFlags, ids: &[String]) -> Result<(), String> {
    let moddir = mod_dir();
    let all = scan_animations(&moddir);

    let selected: Vec<serde_json::Value> = if ids.is_empty() {
        all.into_iter().map(|(_, v)| v).collect()
    } else {
        let mut v = Vec::new();
        for want in ids {
            match all.iter().find(|(id, _)| id == want) {
                Some((_, info)) => v.push(info.clone()),
                None => {
                    let w = format!(
                        "{}: {}",
                        match detect_lang() {
                            Lang::Zh => "未找到动画",
                            Lang::En => "animation not found",
                        },
                        want
                    );
                    log_warn(flags, &w);
                }
            }
        }
        v
    };

    if flags.json {
        json_output(&CmdResult {
            success: true,
            data: Some(InfoData { info: selected }),
            error: None,
        })?;
    } else if selected.is_empty() {
        log_info(
            flags,
            match detect_lang() {
                Lang::Zh => "没有匹配的动画",
                Lang::En => "no matching animations",
            },
        );
    } else {
        for v in &selected {
            let s = serde_json::to_string_pretty(v).map_err(|e| e.to_string())?;
            eprintln!("{}", s);
        }
    }
    Ok(())
}

// ============================================================
// apply
//
// enable=false → 返回 success:false，明确告知原因，不消费 once
// ============================================================
fn apply(flags: &GlobalFlags) -> Result<(), String> {
    let moddir = mod_dir();
    let settings = read_settings(&moddir)?;

    // 禁用状态下直接返回失败，不消费 once
    if !settings.enable {
        let msg = match detect_lang() {
            Lang::Zh => {
                "开机自动生效已被关闭（settings.json: enable=false），跳过挂载".to_string()
            }
            Lang::En => {
                "auto-mount is disabled (settings.json: enable=false), skipping mount".to_string()
            }
        };
        log_warn(flags, &msg);
        if flags.json {
            json_output(&CmdResult::<ApplyData> {
                success: false,
                data: None,
                error: Some(msg),
            })?;
        }
        return Ok(());
    }

    let once_path = moddir.join(ONCE_TXT);
    let once_id = read_once_id(&moddir)?;
    let boot_id = settings
        .bootanimation
        .clone()
        .filter(|s| !s.trim().is_empty());

    let once_valid = once_id
        .as_ref()
        .filter(|i| is_valid_animation(&moddir, i))
        .cloned();
    let boot_valid = boot_id
        .as_ref()
        .filter(|i| is_valid_animation(&moddir, i))
        .cloned();

    let (id, is_once): (String, bool) = if let Some(id) = once_valid {
        (id, true)
    } else {
        if let Some(ref bad) = once_id {
            let _ = fs::remove_file(&once_path);
            let w = format!(
                "{}: {}",
                match detect_lang() {
                    Lang::Zh => "once_bootanimation 指向的动画无效，已删除并 fallback",
                    Lang::En => "once_bootanimation points to invalid animation, removed & fallback",
                },
                bad
            );
            log_warn(flags, &w);
        }
        match boot_valid {
            Some(id) => (id, false),
            None => {
                let msg = if let Some(ref bad) = boot_id {
                    format!(
                        "{}: {}",
                        match detect_lang() {
                            Lang::Zh => "settings.bootanimation 指向的动画无效",
                            Lang::En => "settings.bootanimation points to invalid animation",
                        },
                        bad
                    )
                } else {
                    match detect_lang() {
                        Lang::Zh => "settings.bootanimation 为空，跳过".to_string(),
                        Lang::En => "settings.bootanimation is empty, skipping".to_string(),
                    }
                };
                if boot_id.is_some() {
                    log_error(flags, &msg);
                } else {
                    log_warn(flags, &msg);
                }
                if flags.json {
                    json_output(&CmdResult::<ApplyData> {
                        success: false,
                        data: None,
                        error: Some(msg),
                    })?;
                }
                return Ok(());
            }
        }
    };

    let zip_path = animation_zip(&moddir, &id);

    let sys = match detect_system_animation() {
        Some(s) => s,
        None => {
            let msg = match detect_lang() {
                Lang::Zh => "未检测到系统开机动画路径",
                Lang::En => "system boot animation path not found",
            };
            log_error(flags, msg);
            if flags.json {
                json_output(&CmdResult::<ApplyData> {
                    success: false,
                    data: None,
                    error: Some(msg.into()),
                })?;
            }
            return Ok(());
        }
    };

    if is_mounted(&sys).unwrap_or(false) {
        force_unmount_all(&sys);
        let msg = format!(
            "{}: {}",
            match detect_lang() {
                Lang::Zh => "已清理旧挂载",
                Lang::En => "cleared existing mount",
            },
            sys
        );
        log_info(flags, &msg);
    }

    bind_mount(&zip_path, Path::new(&sys))?;

    let ok_main = match detect_lang() {
        Lang::Zh => format!("已挂载: {} -> {}", zip_path.display(), sys),
        Lang::En => format!("mounted: {} -> {}", zip_path.display(), sys),
    };
    log_ok(flags, &ok_main);

    let mut dark_done = false;
    if let Some(dark) = dark_animation_path(&sys) {
        if is_mounted(&dark).unwrap_or(false) {
            force_unmount_all(&dark);
        }
        if bind_mount(&zip_path, Path::new(&dark)).is_ok() {
            let ok_dark = match detect_lang() {
                Lang::Zh => format!("已挂载 dark: {} -> {}", zip_path.display(), dark),
                Lang::En => format!("mounted dark: {} -> {}", zip_path.display(), dark),
            };
            log_ok(flags, &ok_dark);
            dark_done = true;
        }
    }

    let now_path = moddir.join(NOW_TXT);
    if let Err(e) = fs::write(&now_path, &id) {
        let w = format!(
            "{}: {}",
            match detect_lang() {
                Lang::Zh => format!("写入 {} 失败", now_path.display()),
                Lang::En => format!("failed to write {}", now_path.display()),
            },
            e
        );
        log_warn(flags, &w);
    }

    if is_once {
        let _ = fs::remove_file(&once_path);
    }

    let msg = format!(
        "{}: {}{}{}",
        match detect_lang() {
            Lang::Zh => "成功挂载动画",
            Lang::En => "successfully mounted animation",
        },
        id,
        if dark_done {
            match detect_lang() {
                Lang::Zh => " (含 dark)",
                Lang::En => " (with dark)",
            }
        } else {
            ""
        },
        if is_once {
            match detect_lang() {
                Lang::Zh => " (一次性)",
                Lang::En => " (once)",
            }
        } else {
            ""
        }
    );
    log_ok(flags, &msg);

    if flags.json {
        json_output(&CmdResult {
            success: true,
            data: Some(ApplyData {
                animation_id: Some(id),
                message: msg,
            }),
            error: None,
        })?;
    }
    Ok(())
}

// ============================================================
// apply_temp（无状态，不受 enable 影响）
// ============================================================
fn apply_temp(flags: &GlobalFlags, id: &str) -> Result<(), String> {
    let moddir = mod_dir();

    if !is_valid_animation(&moddir, id) {
        let msg = format!(
            "{}: {}",
            match detect_lang() {
                Lang::Zh => "动画无效（缺少 info.json 或文件不完整）",
                Lang::En => "invalid animation (missing info.json or incomplete)",
            },
            id
        );
        log_error(flags, &msg);
        if flags.json {
            json_output(&CmdResult::<ApplyTempData> {
                success: false,
                data: None,
                error: Some(msg),
            })?;
        }
        return Ok(());
    }

    let zip_path = animation_zip(&moddir, id);

    let sys = match detect_system_animation() {
        Some(s) => s,
        None => {
            let msg = match detect_lang() {
                Lang::Zh => "未检测到系统开机动画路径",
                Lang::En => "system boot animation path not found",
            };
            log_error(flags, msg);
            if flags.json {
                json_output(&CmdResult::<ApplyTempData> {
                    success: false,
                    data: None,
                    error: Some(msg.into()),
                })?;
            }
            return Ok(());
        }
    };

    if is_mounted(&sys).unwrap_or(false) {
        force_unmount_all(&sys);
        let msg = format!(
            "{}: {}",
            match detect_lang() {
                Lang::Zh => "已清理旧挂载",
                Lang::En => "cleared existing mount",
            },
            sys
        );
        log_info(flags, &msg);
    }

    bind_mount(&zip_path, Path::new(&sys))?;

    let ok_main = match detect_lang() {
        Lang::Zh => format!("已临时挂载: {} -> {}", zip_path.display(), sys),
        Lang::En => format!("temporarily mounted: {} -> {}", zip_path.display(), sys),
    };
    log_ok(flags, &ok_main);

    let mut dark_mounted = false;
    if let Some(dark) = dark_animation_path(&sys) {
        if is_mounted(&dark).unwrap_or(false) {
            force_unmount_all(&dark);
        }
        if bind_mount(&zip_path, Path::new(&dark)).is_ok() {
            let ok_dark = match detect_lang() {
                Lang::Zh => format!("已临时挂载 dark: {} -> {}", zip_path.display(), dark),
                Lang::En => format!("temporarily mounted dark: {} -> {}", zip_path.display(), dark),
            };
            log_ok(flags, &ok_dark);
            dark_mounted = true;
        }
    }

    let now_path = moddir.join(NOW_TXT);
    if let Err(e) = fs::write(&now_path, id) {
        let w = format!(
            "{}: {}",
            match detect_lang() {
                Lang::Zh => format!("写入 {} 失败", now_path.display()),
                Lang::En => format!("failed to write {}", now_path.display()),
            },
            e
        );
        log_warn(flags, &w);
    }

    let msg = format!(
        "{} ({} {})",
        match detect_lang() {
            Lang::Zh => format!("临时挂载成功: {}", id),
            Lang::En => format!("temporary mount success: {}", id),
        },
        match detect_lang() {
            Lang::Zh => "重启后失效",
            Lang::En => "lost after reboot",
        },
        if dark_mounted {
            match detect_lang() {
                Lang::Zh => "含 dark",
                Lang::En => "with dark",
            }
        } else {
            ""
        }
    );
    log_ok(flags, &msg);

    if flags.json {
        json_output(&CmdResult {
            success: true,
            data: Some(ApplyTempData {
                animation_id: id.to_string(),
                message: msg,
                main_mounted: true,
                dark_mounted,
            }),
            error: None,
        })?;
    }

    Ok(())
}

// ============================================================
// revert
// ============================================================
fn revert(flags: &GlobalFlags) -> Result<(), String> {
    let moddir = mod_dir();

    let sys = match detect_system_animation() {
        Some(s) => s,
        None => {
            let msg = match detect_lang() {
                Lang::Zh => "未检测到系统开机动画路径",
                Lang::En => "system boot animation path not found",
            };
            log_error(flags, msg);
            if flags.json {
                json_output(&CmdResult::<RevertData> {
                    success: false,
                    data: None,
                    error: Some(msg.into()),
                })?;
            }
            return Ok(());
        }
    };

    let mut unmounted = Vec::new();

    if is_mounted(&sys)? {
        force_unmount_all(&sys);
        let ok_msg = match detect_lang() {
            Lang::Zh => format!("已卸载: {}", sys),
            Lang::En => format!("unmounted: {}", sys),
        };
        log_ok(flags, &ok_msg);
        unmounted.push(sys.clone());
    }

    if let Some(dark) = dark_animation_path(&sys) {
        if is_mounted(&dark)? {
            force_unmount_all(&dark);
            let ok_msg = match detect_lang() {
                Lang::Zh => format!("已卸载: {}", dark),
                Lang::En => format!("unmounted: {}", dark),
            };
            log_ok(flags, &ok_msg);
            unmounted.push(dark);
        }
    }

    let now_path = moddir.join(NOW_TXT);
    if now_path.exists() {
        let _ = fs::remove_file(&now_path);
    }

    let msg: String = if unmounted.is_empty() {
        match detect_lang() {
            Lang::Zh => "没有已挂载的动画".into(),
            Lang::En => "no animation mounted".into(),
        }
    } else {
        match detect_lang() {
            Lang::Zh => "卸载完成".into(),
            Lang::En => "revert complete".into(),
        }
    };
    log_info(flags, &msg);

    if flags.json {
        json_output(&CmdResult {
            success: true,
            data: Some(RevertData {
                unmounted,
                message: msg,
            }),
            error: None,
        })?;
    }
    Ok(())
}

// ============================================================
// status
// ============================================================
fn status(flags: &GlobalFlags) -> Result<(), String> {
    let detected = detect_system_animation_ex();
    let sys = detected.as_ref().map(|(p, _)| p.clone());
    let format = detected.as_ref().map(|(_, is_oppo)| {
        if *is_oppo {
            "coloros".to_string()
        } else {
            "aosp".to_string()
        }
    });

    let moddir = mod_dir();
    let settings = read_settings(&moddir).unwrap_or_default();
    let config_id = settings
        .bootanimation
        .clone()
        .filter(|s| !s.trim().is_empty());
    let enabled = settings.enable;
    let language = settings.language.clone();
    let now_id = read_now_id(&moddir).unwrap_or(None);

    let effective_id = now_id.clone().or_else(|| config_id.clone());

    let module_exists = effective_id
        .as_ref()
        .map(|i| animation_zip(&moddir, i).exists())
        .unwrap_or(false);

    let main_mounted = sys
        .as_ref()
        .map(|s| is_mounted(s).unwrap_or(false))
        .unwrap_or(false);

    let dark_mounted = sys
        .as_ref()
        .and_then(|s| dark_animation_path(s))
        .map(|d| is_mounted(&d).unwrap_or(false))
        .unwrap_or(false);

    let main_sources = sys
        .as_ref()
        .and_then(|s| mounted_sources(s).ok())
        .unwrap_or_default();

    let data = StatusData {
        system_path: sys.clone(),
        system_format: format.clone(),
        enabled,
        language: language.clone(),
        animation_id: effective_id.clone(),
        now_animation_id: now_id.clone(),
        config_animation_id: config_id.clone(),
        module_file_exists: module_exists,
        main_mounted,
        dark_mounted,
        main_sources: main_sources.clone(),
    };

    if flags.json {
        json_output(&CmdResult {
            success: true,
            data: Some(&data),
            error: None,
        })?;
    } else {
        let l_sys = match detect_lang() {
            Lang::Zh => "系统动画路径",
            Lang::En => "system_path",
        };
        let l_fmt = match detect_lang() {
            Lang::Zh => "系统格式",
            Lang::En => "system_format",
        };
        let l_en = match detect_lang() {
            Lang::Zh => "启用状态 (enable)",
            Lang::En => "enabled",
        };
        let l_lang = match detect_lang() {
            Lang::Zh => "语言 (language)",
            Lang::En => "language",
        };
        let l_eff = match detect_lang() {
            Lang::Zh => "实际生效 ID",
            Lang::En => "effective_id",
        };
        let l_now = match detect_lang() {
            Lang::Zh => "当前已挂载 ID (now)",
            Lang::En => "now_animation_id",
        };
        let l_cfg = match detect_lang() {
            Lang::Zh => "配置 ID (settings.bootanimation)",
            Lang::En => "config_animation_id",
        };
        let l_mod = match detect_lang() {
            Lang::Zh => "模块文件存在",
            Lang::En => "module_file_exists",
        };
        let l_main = match detect_lang() {
            Lang::Zh => "主动画挂载",
            Lang::En => "main_mounted",
        };
        let l_dark = match detect_lang() {
            Lang::Zh => "Dark 动画",
            Lang::En => "dark_mounted",
        };
        let none_str = match detect_lang() {
            Lang::Zh => "(未设置)",
            Lang::En => "(not set)",
        };
        let sys_str = sys.as_deref().unwrap_or(match detect_lang() {
            Lang::Zh => "(未检测到)",
            Lang::En => "(not found)",
        });
        let fmt_str = format.as_deref().unwrap_or("(unknown)");
        let lang_str = language.as_deref().unwrap_or(match detect_lang() {
            Lang::Zh => "(自动检测)",
            Lang::En => "(auto-detect)",
        });
        let eff_str = effective_id.as_deref().unwrap_or(none_str);
        let now_str = now_id.as_deref().unwrap_or(none_str);
        let cfg_str = config_id.as_deref().unwrap_or(none_str);
        let sources_str = if !main_sources.is_empty() {
            format!(" ({})", main_sources.join(", "))
        } else {
            String::new()
        };
        eprintln!("{}: {}", l_sys, sys_str);
        eprintln!("{}: {}", l_fmt, fmt_str);
        eprintln!("{}: {}", l_en, enabled);
        eprintln!("{}: {}", l_lang, lang_str);
        eprintln!("{}: {}", l_eff, eff_str);
        eprintln!("{}: {}", l_now, now_str);
        eprintln!("{}: {}", l_cfg, cfg_str);
        eprintln!("{}: {}", l_mod, module_exists);
        eprintln!("{}: {}{}", l_main, main_mounted, sources_str);
        eprintln!("{}: {}", l_dark, dark_mounted);
    }

    Ok(())
}

// ============================================================
// check
// ============================================================
fn check(flags: &GlobalFlags) -> Result<(), String> {
    let moddir = mod_dir();

    let settings = read_settings(&moddir).unwrap_or_default();
    let config_id = settings
        .bootanimation
        .clone()
        .filter(|s| !s.trim().is_empty());
    let enabled = settings.enable;
    let now_id = read_now_id(&moddir)?;

    let effective_id = now_id.clone().or_else(|| config_id.clone());

    let id = match effective_id {
        Some(id) => id,
        None => {
            let msg = match detect_lang() {
                Lang::Zh => "没有可检查的动画（now_bootanimation 和 settings.bootanimation 都为空）",
                Lang::En => "no animation to check (both now_bootanimation and settings.bootanimation are empty)",
            };
            log_warn(flags, msg);
            if flags.json {
                json_output(&CmdResult::<CheckData> {
                    success: false,
                    data: None,
                    error: Some(msg.into()),
                })?;
            }
            return Ok(());
        }
    };

    let module_zip = animation_zip(&moddir, &id);
    let sys = match detect_system_animation() {
        Some(s) => s,
        None => {
            let msg = match detect_lang() {
                Lang::Zh => "未检测到系统开机动画路径",
                Lang::En => "system boot animation path not found",
            };
            log_error(flags, msg);
            if flags.json {
                json_output(&CmdResult::<CheckData> {
                    success: false,
                    data: None,
                    error: Some(msg.into()),
                })?;
            }
            return Ok(());
        }
    };

    let module_hash = file_sha256(module_zip.to_str().unwrap())?;
    let system_hash = file_sha256(&sys)?;
    let mounted = is_mounted(&sys)?;

    let module_exists = module_hash.is_some();
    let system_exists = system_hash.is_some();

    let active = matches!(
        (&module_hash, &system_hash, mounted),
        (Some(m), Some(s), true) if m == s
    );

    let message: String = if active {
        match detect_lang() {
            Lang::Zh => "动画已生效".into(),
            Lang::En => "animation is active".into(),
        }
    } else if !module_exists {
        match detect_lang() {
            Lang::Zh => "模块动画不存在".into(),
            Lang::En => "module animation not found".into(),
        }
    } else if !system_exists {
        match detect_lang() {
            Lang::Zh => "系统动画不存在".into(),
            Lang::En => "system animation not found".into(),
        }
    } else if !mounted {
        match detect_lang() {
            Lang::Zh => "未挂载".into(),
            Lang::En => "not mounted".into(),
        }
    } else {
        match (now_id.as_ref(), config_id.as_ref()) {
            (Some(_), _) => match detect_lang() {
                Lang::Zh => "系统实际挂载的动画与 now_bootanimation 不一致".into(),
                Lang::En => "actual mounted animation differs from now_bootanimation".into(),
            },
            (None, Some(_)) => match detect_lang() {
                Lang::Zh => "系统实际挂载的动画与 settings.bootanimation 不一致".into(),
                Lang::En => "actual mounted animation differs from settings.bootanimation".into(),
            },
            _ => match detect_lang() {
                Lang::Zh => "哈希不一致（可能被覆盖）".into(),
                Lang::En => "hash mismatch (may be overridden)".into(),
            },
        }
    };

    let data = CheckData {
        animation_id: Some(id.clone()),
        now_animation_id: now_id.clone(),
        config_animation_id: config_id.clone(),
        enabled,
        module_hash: module_hash.clone(),
        system_hash: system_hash.clone(),
        module_exists,
        system_exists,
        mounted,
        active,
        message: message.clone(),
    };

    if flags.json {
        json_output(&CmdResult {
            success: true,
            data: Some(&data),
            error: None,
        })?;
    } else {
        let l_id = match detect_lang() {
            Lang::Zh => "比对 ID",
            Lang::En => "checking_id",
        };
        let l_now = match detect_lang() {
            Lang::Zh => "now_bootanimation",
            Lang::En => "now_animation_id",
        };
        let l_cfg = match detect_lang() {
            Lang::Zh => "settings.bootanimation",
            Lang::En => "config_animation_id",
        };
        let l_en = match detect_lang() {
            Lang::Zh => "enable",
            Lang::En => "enabled",
        };
        let l_modh = match detect_lang() {
            Lang::Zh => "模块动画哈希",
            Lang::En => "module_hash",
        };
        let l_sysh = match detect_lang() {
            Lang::Zh => "系统动画哈希",
            Lang::En => "system_hash",
        };
        let l_mnt = match detect_lang() {
            Lang::Zh => "挂载状态",
            Lang::En => "mounted",
        };
        let l_sta = match detect_lang() {
            Lang::Zh => "状态",
            Lang::En => "status",
        };
        let l_reason = match detect_lang() {
            Lang::Zh => "原因",
            Lang::En => "reason",
        };
        let none_str = match detect_lang() {
            Lang::Zh => "(未设置)",
            Lang::En => "(not set)",
        };
        let notfound_str = match detect_lang() {
            Lang::Zh => "(不存在)",
            Lang::En => "(not found)",
        };
        let mod_h = module_hash
            .as_ref()
            .map(|h| h[..16].to_string())
            .unwrap_or_else(|| notfound_str.into());
        let sys_h = system_hash
            .as_ref()
            .map(|h| h[..16].to_string())
            .unwrap_or_else(|| notfound_str.into());
        eprintln!("{}: {}", l_id, id);
        eprintln!("{}: {}", l_now, now_id.as_deref().unwrap_or(none_str));
        eprintln!("{}: {}", l_cfg, config_id.as_deref().unwrap_or(none_str));
        eprintln!("{}: {}", l_en, enabled);
        eprintln!("{}: {}", l_modh, mod_h);
        eprintln!("{}: {}", l_sysh, sys_h);
        eprintln!("{}: {}", l_mnt, mounted);
        let status_str = if active {
            match detect_lang() {
                Lang::Zh => "动画已生效",
                Lang::En => "active",
            }
        } else {
            match detect_lang() {
                Lang::Zh => "动画未生效",
                Lang::En => "inactive",
            }
        };
        eprintln!("{}: {}", l_sta, status_str);
        eprintln!("{}: {}", l_reason, message);
    }

    Ok(())
}

// ============================================================
// use（持久化配置）
// ============================================================
fn use_cmd(flags: &GlobalFlags, id: &str) -> Result<(), String> {
    let moddir = mod_dir();

    if !is_valid_animation(&moddir, id) {
        let msg = format!(
            "{}: {}",
            match detect_lang() {
                Lang::Zh => "动画无效（缺少 info.json 或文件不完整）",
                Lang::En => "invalid animation (missing info.json or incomplete)",
            },
            id
        );
        log_error(flags, &msg);
        if flags.json {
            json_output(&CmdResult::<UseData> {
                success: false,
                data: None,
                error: Some(msg),
            })?;
        }
        return Ok(());
    }

    let mut settings = read_settings(&moddir)?;
    settings.bootanimation = Some(id.to_string());
    write_settings(&moddir, &settings)?;

    let msg = format!(
        "{}: {}",
        match detect_lang() {
            Lang::Zh => "已设置默认动画",
            Lang::En => "default animation set",
        },
        id
    );
    log_ok(flags, &msg);

    if flags.json {
        json_output(&CmdResult {
            success: true,
            data: Some(UseData {
                animation_id: id.to_string(),
                message: msg,
            }),
            error: None,
        })?;
    }

    Ok(())
}

// ============================================================
// once_apply（一次性动画）
// ============================================================
fn once_apply_cmd(flags: &GlobalFlags, id: &str) -> Result<(), String> {
    let moddir = mod_dir();

    if !is_valid_animation(&moddir, id) {
        let msg = format!(
            "{}: {}",
            match detect_lang() {
                Lang::Zh => "动画无效（缺少 info.json 或文件不完整）",
                Lang::En => "invalid animation (missing info.json or incomplete)",
            },
            id
        );
        log_error(flags, &msg);
        if flags.json {
            json_output(&CmdResult::<UseData> {
                success: false,
                data: None,
                error: Some(msg),
            })?;
        }
        return Ok(());
    }

    let path = moddir.join(ONCE_TXT);
    fs::write(&path, id)
        .map_err(|e| format!("写入 {} 失败: {}", path.display(), e))?;

    let msg = format!(
        "{}: {}",
        match detect_lang() {
            Lang::Zh => "已设置一次性动画（下次 apply 时使用一次，用后删除）",
            Lang::En => "once animation set (used once on next apply, then removed)",
        },
        id
    );
    log_ok(flags, &msg);

    if flags.json {
        json_output(&CmdResult {
            success: true,
            data: Some(UseData {
                animation_id: id.to_string(),
                message: msg,
            }),
            error: None,
        })?;
    }

    Ok(())
}

// ============================================================
// enable / disable
// ============================================================
fn set_enable(flags: &GlobalFlags, value: bool) -> Result<(), String> {
    let moddir = mod_dir();
    let mut settings = read_settings(&moddir)?;
    settings.enable = value;
    write_settings(&moddir, &settings)?;

    let msg = if value {
        match detect_lang() {
            Lang::Zh => "已启用开机自动挂载（enable=true）".to_string(),
            Lang::En => "auto-mount enabled (enable=true)".to_string(),
        }
    } else {
        match detect_lang() {
            Lang::Zh => "已禁用开机自动挂载（enable=false）".to_string(),
            Lang::En => "auto-mount disabled (enable=false)".to_string(),
        }
    };
    log_ok(flags, &msg);

    if flags.json {
        json_output(&CmdResult {
            success: true,
            data: Some(ToggleData {
                enabled: value,
                message: msg,
            }),
            error: None,
        })?;
    }
    Ok(())
}

fn enable_cmd(flags: &GlobalFlags) -> Result<(), String> {
    set_enable(flags, true)
}

fn disable_cmd(flags: &GlobalFlags) -> Result<(), String> {
    set_enable(flags, false)
}

// ============================================================
// install 相关
// ============================================================
#[derive(serde::Deserialize)]
struct ConfigPart {
    #[serde(rename = "type")]
    part_type: Option<String>,
    #[serde(rename = "loop")]
    loop_count: Option<u32>,
    pause: Option<u32>,
    path: String,
}

#[derive(serde::Deserialize)]
struct Config {
    fps: Option<u32>,
    width: Option<u32>,
    height: Option<u32>,
    offset_x: Option<i32>,
    offset_y: Option<i32>,
    part: Vec<ConfigPart>,
}

#[derive(serde::Deserialize)]
struct Info {
    author: Option<String>,
    name: Option<String>,
    id: String,
    desc: Option<String>,
    version: Option<String>,
    #[serde(rename = "version_code")]
    version_code: Option<u32>,
}

fn detect_resolution() -> (u32, u32) {
    let props = [
        ("ro.product.display.width", "ro.product.display.height"),
        ("ro.display.width", "ro.display.height"),
        ("persist.sys.display.width", "persist.sys.display.height"),
    ];
    for (wp, hp) in &props {
        let w = std::process::Command::new("getprop")
            .arg(wp)
            .output()
            .ok()
            .and_then(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .trim()
                    .parse::<u32>()
                    .ok()
            });
        let h = std::process::Command::new("getprop")
            .arg(hp)
            .output()
            .ok()
            .and_then(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .trim()
                    .parse::<u32>()
                    .ok()
            });
        if let (Some(w), Some(h)) = (w, h) {
            if w > 0 && h > 0 {
                return (w, h);
            }
        }
    }
    if let Ok(s) = fs::read_to_string("/sys/class/graphics/fb0/virtual_size") {
        let p: Vec<&str> = s.trim().split(',').collect();
        if p.len() >= 2 {
            if let (Ok(w), Ok(h)) = (p[0].trim().parse::<u32>(), p[1].trim().parse::<u32>()) {
                if w > 0 && h > 0 {
                    return (w, h);
                }
            }
        }
    }
    (1080, 2400)
}

fn generate_desc(config: &Config, w: u32, h: u32, coloros: bool) -> String {
    let fps = config.fps.unwrap_or(60);
    let mut d = if coloros {
        let ox = config.offset_x.unwrap_or(0);
        let oy = config.offset_y.unwrap_or(0);
        format!("g {} {} {} {} {}\n", w, h, ox, oy, fps)
    } else {
        format!("{} {} {}\n", w, h, fps)
    };
    for p in &config.part {
        d.push_str(&format!(
            "{} {} {} {}\n",
            p.part_type.as_deref().unwrap_or("p"),
            p.loop_count.unwrap_or(0),
            p.pause.unwrap_or(0),
            p.path
        ));
    }
    d.push('\n');
    d
}

fn is_image_file(path: &Path) -> bool {
    match path.extension().and_then(|e| e.to_str()) {
        Some(ext) => {
            let e = ext.to_lowercase();
            e == "jpg" || e == "jpeg" || e == "png"
        }
        None => false,
    }
}

/// 显式写入目录条目，匹配 `zip -rq -0` 的行为，
/// 避免 Android / ColorOS bootanimation 解析器因缺少目录条目而异常。
fn add_dir_to_zip(
    zip: &mut zip_crate::ZipWriter<fs::File>,
    dir: &Path,
    prefix: &str,
    opts: zip_crate::write::FileOptions,
) -> Result<(), String> {
    for e in fs::read_dir(dir).map_err(|e| e.to_string())? {
        let e = e.map_err(|e| e.to_string())?;
        let p = e.path();
        let name = p.file_name().unwrap().to_string_lossy();
        let zp = format!("{}{}", prefix, name);
        if p.is_dir() {
            zip.add_directory(format!("{}/", zp), opts)
                .map_err(|e| e.to_string())?;
            add_dir_to_zip(zip, &p, &format!("{}/", zp), opts)?;
        } else {
            if !is_image_file(&p) {
                continue;
            }
            zip.start_file(&zp, opts).map_err(|e| e.to_string())?;
            let data = fs::read(&p).map_err(|e| e.to_string())?;
            zip.write_all(&data).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn make_zip(
    src: &Path,
    config: &Config,
    out: &Path,
    coloros: bool,
) -> Result<(), String> {
    let (w, h) = if config.width.is_some() && config.height.is_some() {
        (config.width.unwrap(), config.height.unwrap())
    } else {
        detect_resolution()
    };

    let desc = generate_desc(config, w, h, coloros);
    let file = fs::File::create(out).map_err(|e| e.to_string())?;
    let mut zip = zip_crate::ZipWriter::new(file);
    let opts = zip_crate::write::FileOptions::default()
        .compression_method(zip_crate::CompressionMethod::Stored);

    zip.start_file("desc.txt", opts).map_err(|e| e.to_string())?;
    zip.write_all(desc.as_bytes()).map_err(|e| e.to_string())?;

    let parts = src.join("parts");
    if !parts.exists() {
        return Err(match detect_lang() {
            Lang::Zh => "parts 目录不存在".into(),
            Lang::En => "parts directory not found".into(),
        });
    }
    for e in fs::read_dir(&parts).map_err(|e| e.to_string())? {
        let e = e.map_err(|e| e.to_string())?;
        let p = e.path();
        if p.is_dir() {
            let n = p.file_name().unwrap().to_string_lossy();
            zip.add_directory(format!("{}/", n), opts)
                .map_err(|e| e.to_string())?;
            add_dir_to_zip(&mut zip, &p, &format!("{}/", n), opts)?;
        }
    }
    zip.finish().map_err(|e| e.to_string())?;
    Ok(())
}

fn extract_zip(src: &str, dest: &Path) -> Result<(), String> {
    let f = fs::File::open(src).map_err(|e| e.to_string())?;
    let mut ar = zip_crate::ZipArchive::new(f).map_err(|e| e.to_string())?;
    for i in 0..ar.len() {
        let mut f = ar.by_index(i).map_err(|e| e.to_string())?;
        let out = dest.join(f.name());
        if f.name().ends_with('/') {
            fs::create_dir_all(&out).map_err(|e| e.to_string())?;
        } else {
            if let Some(p) = out.parent() {
                fs::create_dir_all(p).map_err(|e| e.to_string())?;
            }
            let mut o = fs::File::create(&out).map_err(|e| e.to_string())?;
            std::io::copy(&mut f, &mut o).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

// ============================================================
// install
// ============================================================
fn install(flags: &GlobalFlags, zip_path: &str) -> Result<(), String> {
    let moddir = mod_dir();
    let tmp = moddir.join("temp_install");

    if tmp.exists() {
        fs::remove_dir_all(&tmp).map_err(|e| e.to_string())?;
    }
    fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;

    let extract_msg = format!(
        "{}: {}",
        match detect_lang() {
            Lang::Zh => "解压",
            Lang::En => "extracting",
        },
        zip_path
    );
    log_info(flags, &extract_msg);
    extract_zip(zip_path, &tmp)?;

    let detected = detect_system_animation_ex();
    let coloros = detected
        .as_ref()
        .map(|(_, is_oppo)| *is_oppo)
        .unwrap_or(false);
    let system_format = if coloros { "coloros" } else { "aosp" };

    if coloros {
        let m = match detect_lang() {
            Lang::Zh => "检测到 ColorOS 路径，desc.txt 将使用 g 格式",
            Lang::En => "ColorOS path detected, desc.txt will use g format",
        };
        log_info(flags, m);
    } else {
        let m = match detect_lang() {
            Lang::Zh => "检测到 AOSP 路径，desc.txt 将使用标准格式",
            Lang::En => "AOSP path detected, desc.txt will use standard format",
        };
        log_info(flags, m);
    }

    let current_now_id = read_now_id(&moddir).unwrap_or(None);

    let mut installed = Vec::new();
    let mut updated = Vec::new();
    let mut skipped = Vec::new();

    for e in fs::read_dir(&tmp).map_err(|e| e.to_string())? {
        let e = e.map_err(|e| e.to_string())?;
        let path = e.path();
        if !path.is_dir() {
            continue;
        }

        let folder = path.file_name().unwrap().to_string_lossy().into_owned();

        let info_path = path.join("info.json");
        if !info_path.exists() {
            let w = format!(
                "{} {}: {}",
                match detect_lang() {
                    Lang::Zh => "跳过",
                    Lang::En => "skip",
                },
                folder,
                match detect_lang() {
                    Lang::Zh => "无 info.json",
                    Lang::En => "no info.json",
                }
            );
            log_warn(flags, &w);
            skipped.push(folder);
            continue;
        }

        let info: Info = serde_json::from_str(
            &fs::read_to_string(&info_path).map_err(|e| e.to_string())?,
        )
        .map_err(|e| format!("{} info.json {}", folder, e))?;

        if info.id != folder {
            let w = format!(
                "{} {}: id='{}' {}",
                match detect_lang() {
                    Lang::Zh => "拒载",
                    Lang::En => "reject",
                },
                folder,
                info.id,
                match detect_lang() {
                    Lang::Zh => "与目录名不一致",
                    Lang::En => "mismatch folder name",
                }
            );
            log_error(flags, &w);
            skipped.push(folder);
            continue;
        }

        let cfg_path = path.join("config.json");
        if !cfg_path.exists() {
            let w = format!(
                "{} {}: {}",
                match detect_lang() {
                    Lang::Zh => "跳过",
                    Lang::En => "skip",
                },
                info.id,
                match detect_lang() {
                    Lang::Zh => "无 config.json",
                    Lang::En => "no config.json",
                }
            );
            log_warn(flags, &w);
            skipped.push(info.id.clone());
            continue;
        }

        let config: Config = serde_json::from_str(
            &fs::read_to_string(&cfg_path).map_err(|e| e.to_string())?,
        )
        .map_err(|e| format!("{} config.json {}", info.id, e))?;

        let target = moddir.join(ANIM_DIR).join(&info.id);
        let is_update = target.exists();

        let mut was_active = false;
        if is_update {
            if let Some(ref cur) = current_now_id {
                if cur == &info.id {
                    was_active = true;
                    if let Some(sys) = detect_system_animation() {
                        if is_mounted(&sys).unwrap_or(false) {
                            force_unmount_all(&sys);
                            let ok = format!(
                                "{}: {}",
                                match detect_lang() {
                                    Lang::Zh => "已卸载旧版本",
                                    Lang::En => "unmounted old version",
                                },
                                sys
                            );
                            log_ok(flags, &ok);
                        }
                        if let Some(dark) = dark_animation_path(&sys) {
                            if is_mounted(&dark).unwrap_or(false) {
                                force_unmount_all(&dark);
                            }
                        }
                    }
                }
            }
        }

        if target.exists() {
            fs::remove_dir_all(&target).map_err(|e| e.to_string())?;
        }
        fs::create_dir_all(&target).map_err(|e| e.to_string())?;

        let out_zip = target.join("bootanimation.zip");
        let proc_msg = format!(
            "{}: {}",
            if is_update {
                match detect_lang() {
                    Lang::Zh => "更新",
                    Lang::En => "updating",
                }
            } else {
                match detect_lang() {
                    Lang::Zh => "安装",
                    Lang::En => "installing",
                }
            },
            info.id
        );
        log_info(flags, &proc_msg);
        make_zip(&path, &config, &out_zip, coloros)?;

        fs::copy(&info_path, target.join("info.json")).map_err(|e| e.to_string())?;

        if was_active {
            if let Some(sys) = detect_system_animation() {
                if !is_mounted(&sys).unwrap_or(false) {
                    if bind_mount(&out_zip, Path::new(&sys)).is_ok() {
                        let ok = format!(
                            "{}: {} -> {}",
                            match detect_lang() {
                                Lang::Zh => "已重新挂载",
                                Lang::En => "remounted",
                            },
                            out_zip.display(),
                            sys
                        );
                        log_ok(flags, &ok);

                        if let Some(dark) = dark_animation_path(&sys) {
                            let _ = bind_mount(&out_zip, Path::new(&dark));
                        }
                    }
                }
            }
        }

        let name = info.name.as_deref().unwrap_or(&info.id);
        if is_update {
            let ok = format!(
                "{}: {}",
                match detect_lang() {
                    Lang::Zh => "更新成功",
                    Lang::En => "updated",
                },
                name
            );
            log_ok(flags, &ok);
            updated.push(info.id.clone());
        } else {
            let ok = format!(
                "{}: {}",
                match detect_lang() {
                    Lang::Zh => "安装成功",
                    Lang::En => "installed",
                },
                name
            );
            log_ok(flags, &ok);
            installed.push(info.id.clone());
        }
    }

    let _ = fs::remove_dir_all(&tmp);

    // 首次安装且 settings.bootanimation 为空 → 设置为第一个安装的动画
    if !installed.is_empty() && updated.is_empty() {
        let mut settings = read_settings(&moddir).unwrap_or_default();
        let empty = settings
            .bootanimation
            .as_ref()
            .map(|s| s.trim().is_empty())
            .unwrap_or(true);
        if empty {
            settings.bootanimation = Some(installed[0].clone());
            let _ = write_settings(&moddir, &settings);
            let info = format!(
                "{}: {}",
                match detect_lang() {
                    Lang::Zh => "默认动画",
                    Lang::En => "default animation",
                },
                installed[0]
            );
            log_info(flags, &info);
        }
    }

    let msg = format!(
        "{} {}, {} {}, {} {} (format: {})",
        match detect_lang() {
            Lang::Zh => "安装",
            Lang::En => "installed",
        },
        installed.len(),
        match detect_lang() {
            Lang::Zh => "更新",
            Lang::En => "updated",
        },
        updated.len(),
        match detect_lang() {
            Lang::Zh => "跳过",
            Lang::En => "skipped",
        },
        skipped.len(),
        system_format
    );
    log_info(flags, &msg);

    if flags.json {
        json_output(&CmdResult {
            success: true,
            data: Some(InstallData {
                system_format: system_format.to_string(),
                installed,
                updated,
                skipped,
                message: msg,
            }),
            error: None,
        })?;
    }

    Ok(())
}

// ============================================================
// help
// ============================================================
fn print_help(prog: &str) {
    let h = match detect_lang() {
        Lang::Zh => "用法",
        Lang::En => "Usage",
    };
    let c = match detect_lang() {
        Lang::Zh => "命令",
        Lang::En => "Commands",
    };
    let o = match detect_lang() {
        Lang::Zh => "全局选项",
        Lang::En => "Global Options",
    };
    let j = match detect_lang() {
        Lang::Zh => "以 JSON 格式输出到 stdout",
        Lang::En => "output JSON to stdout",
    };
    let l = match detect_lang() {
        Lang::Zh => "JSON 模式下同时输出日志到 stderr",
        Lang::En => "also output logs to stderr in JSON mode",
    };
    let all_json = match detect_lang() {
        Lang::Zh => "所有命令均支持 --json",
        Lang::En => "all commands support --json",
    };
    eprintln!(
        r#"
{help} {prog} <command> [options]

{cmd}:
  apply                      {apply_zh}
  apply --temp <id>          {temp_zh}
  revert                     {revert_zh}
  status                     {status_zh}
  check                      {check_zh}
  use <id>                   {use_zh}
  once_apply <id>            {once_zh}
  enable                     {enable_zh}
  disable                    {disable_zh}
  install <zip>              {install_zh}
  list                       {list_zh}
  info [id...]               {info_zh}
  help                       {help_zh}

{opt}:
  --json                     {json}
  --logs                     {logs}

{all}

settings.json 格式：
  {{
    "bootanimation": "id",
    "enable": true,
    "language": "zh_CN"
  }}
  - enable=false 时：
      * apply（含 post-fs-data.sh 自动调用）返回 success:false，提示"已禁用"
      * once_apply / apply --temp 不受影响
      * once_bootanimation 保留，待 enable=true 后消费
  - language 为最高优先级的界面语言（zh* → 中文，en* → 英文），
    不存在时才使用环境变量检测

ID 文件角色：
  once_bootanimation  →  一次性，apply 成功后自动删除
  settings.json       →  持久化配置（bootanimation + enable + language）
  now_bootanimation   →  当前实际生效的（apply / apply_temp 成功后写入）

如何判断当前是 once/temp 挂载：
  对比 status 输出的 now_animation_id 与 config_animation_id
    一致       →  持久化模式
    不一致     →  once_apply 或 apply --temp 挂的
    now 为空   →  未挂载 / 已 revert
"#,
        help = h,
        prog = prog,
        cmd = c,
        opt = o,
        apply_zh = match detect_lang() {
            Lang::Zh => "挂载动画（enable=false 时返回失败；once → settings 优先）",
            Lang::En => "mount animation (returns failure when enable=false; once → settings priority)",
        },
        temp_zh = match detect_lang() {
            Lang::Zh => "临时挂载指定动画（无状态，不受 enable 影响，重启失效）",
            Lang::En => "temporarily mount an animation (stateless, ignores enable, lost on reboot)",
        },
        revert_zh = match detect_lang() {
            Lang::Zh => "卸载所有挂载层并删除 now_bootanimation",
            Lang::En => "unmount all layers and remove now_bootanimation",
        },
        status_zh = match detect_lang() {
            Lang::Zh => "查看状态（enable / language / now / config）",
            Lang::En => "show status (enable / language / now / config)",
        },
        check_zh = match detect_lang() {
            Lang::Zh => "检查动画是否生效（比对 now 优先，fallback config）",
            Lang::En => "check if animation is active (now priority, fallback config)",
        },
        use_zh = match detect_lang() {
            Lang::Zh => "设置持久化默认动画（写 settings.bootanimation）",
            Lang::En => "set persistent default animation (writes settings.bootanimation)",
        },
        once_zh = match detect_lang() {
            Lang::Zh => "设置一次性动画（写 once_bootanimation，下次 apply 用一次后删除）",
            Lang::En => "set once animation (writes once_bootanimation, used once on next apply)",
        },
        enable_zh = match detect_lang() {
            Lang::Zh => "启用开机自动挂载（settings.enable=true）",
            Lang::En => "enable auto-mount (settings.enable=true)",
        },
        disable_zh = match detect_lang() {
            Lang::Zh => "禁用开机自动挂载（settings.enable=false，apply 会失败）",
            Lang::En => "disable auto-mount (settings.enable=false; apply fails)",
        },
        install_zh = match detect_lang() {
            Lang::Zh => "安装动画包（自动识别 AOSP / ColorOS 格式）",
            Lang::En => "install animation package (auto-detect AOSP / ColorOS)",
        },
        list_zh = match detect_lang() {
            Lang::Zh => "列出所有动画的 ID 和名称",
            Lang::En => "list all animation IDs and names",
        },
        info_zh = match detect_lang() {
            Lang::Zh => "列出动画完整信息（可指定一个或多个 ID）",
            Lang::En => "show full animation info (optionally filter by one or more IDs)",
        },
        help_zh = match detect_lang() {
            Lang::Zh => "显示帮助",
            Lang::En => "show help",
        },
        json = j,
        logs = l,
        all = all_json,
    );
}

// ============================================================
// main
// ============================================================
fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args
        .get(0)
        .map(|s| {
            Path::new(s)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        })
        .unwrap_or_else(|| "utils".into());

    let mut flags = GlobalFlags::default();
    let mut cmd_args = Vec::new();

    for a in args.iter().skip(1) {
        match a.as_str() {
            "--json" => flags.json = true,
            "--logs" => flags.logs = true,
            _ => cmd_args.push(a.clone()),
        }
    }

    let cmd = cmd_args.get(0).map(|s| s.as_str());
    let sub = &cmd_args[1..];

    let result: Result<(), String> = match cmd {
        Some("apply") => {
            if let Some(pos) = sub.iter().position(|a| a == "--temp") {
                if let Some(id) = sub.get(pos + 1) {
                    apply_temp(&flags, id)
                } else {
                    Err(match detect_lang() {
                        Lang::Zh => "用法: utils apply --temp <id>".into(),
                        Lang::En => "usage: utils apply --temp <id>".into(),
                    })
                }
            } else {
                apply(&flags)
            }
        }
        Some("revert") => revert(&flags),
        Some("status") => status(&flags),
        Some("check") => check(&flags),
        Some("use") => {
            if let Some(id) = sub.get(0) {
                use_cmd(&flags, id)
            } else {
                Err(match detect_lang() {
                    Lang::Zh => "用法: utils use <id>".into(),
                    Lang::En => "usage: utils use <id>".into(),
                })
            }
        }
        Some("once_apply") => {
            if let Some(id) = sub.get(0) {
                once_apply_cmd(&flags, id)
            } else {
                Err(match detect_lang() {
                    Lang::Zh => "用法: utils once_apply <id>".into(),
                    Lang::En => "usage: utils once_apply <id>".into(),
                })
            }
        }
        Some("enable") => enable_cmd(&flags),
        Some("disable") => disable_cmd(&flags),
        Some("install") => {
            if let Some(z) = sub.get(0) {
                install(&flags, z)
            } else {
                Err(match detect_lang() {
                    Lang::Zh => "用法: utils install <zip>".into(),
                    Lang::En => "usage: utils install <zip>".into(),
                })
            }
        }
        Some("list") => list_cmd(&flags),
        Some("info") => info_cmd(&flags, sub),
        Some("help") | Some("--help") | Some("-h") => {
            print_help(&prog);
            exit(0);
        }
        _ => {
            print_help(&prog);
            exit(1);
        }
    };

    if let Err(e) = result {
        if flags.json {
            let _ = json_output(&CmdResult::<()> {
                success: false,
                data: None,
                error: Some(e),
            });
        } else {
            log_error(&flags, &e);
        }
        exit(1);
    }
}