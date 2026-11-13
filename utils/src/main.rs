use std::env;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::exit;

use hex::encode as hex_encode;
use rustix::mount::{mount, umount, MountFlags};
use sha2::{Digest, Sha256};
use zip as zip_crate;

use serde::Serialize;

// ============================================================
// 常量
// ============================================================
const BOOT_TXT: &str = "bootanimation.txt";
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
// 语言检测
// ============================================================
#[derive(Clone, Copy, PartialEq)]
enum Lang {
    Zh,
    En,
}

fn detect_lang() -> Lang {
    // INFO_ENG 强制英文
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
// 路径工具
// ============================================================
fn mod_dir() -> PathBuf {
    // 优先用外部传入的 MODDIR
    if let Ok(dir) = env::var("MODDIR") {
        if !dir.is_empty() {
            return PathBuf::from(dir);
        }
    }
    // fallback: 当前工作目录
    env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn read_boot_id(moddir: &Path) -> Result<Option<String>, String> {
    let path = moddir.join(BOOT_TXT);
    let file = match File::open(&path) {
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

fn animation_zip(moddir: &Path, id: &str) -> PathBuf {
    moddir.join(ANIM_DIR).join(id).join("bootanimation.zip")
}

fn detect_system_animation() -> Option<String> {
    for p in SYSTEM_CANDIDATES {
        if Path::new(p).exists() {
            return Some(p.to_string());
        }
    }
    for p in COLOROS_CANDIDATES {
        if Path::new(p).exists() {
            return Some(p.to_string());
        }
    }
    None
}

fn dark_animation_path(sys: &str) -> Option<String> {
    let p = Path::new(sys).parent()?.join("bootanimation-dark.zip");
    p.exists().then(|| p.to_string_lossy().into_owned())
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
        .map_err(|_| "无法读取 /proc/mounts".into())?;
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
    let target = target.to_string_lossy();
    Ok(parse_proc_mounts()?.iter().any(|(_, m)| m == target))
}

fn mounted_sources(target: &str) -> Result<Vec<String>, String> {
    let target = if let Ok(c) = fs::canonicalize(target) {
        c
    } else {
        return Ok(vec);
    };
    let target = target.to_string_lossy();
    Ok(parse_proc_mounts()?
        .iter()
        .filter_map(|(s, m)| if m == target { Some(s.clone()) } else { None })
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
    animation_id: Option<String>,
    module_file_exists: bool,
    main_mounted: bool,
    dark_mounted: bool,
    main_sources: Vec<String>,
}

#[derive(Serialize)]
struct CheckData {
    animation_id: Option<String>,
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
struct ApplyTempData {
    animation_id: String,
    message: String,
    main_mounted: bool,
    dark_mounted: bool,
}

#[derive(Serialize)]
struct InstallData {
    installed: Vec<String>,
    updated: Vec<String>,
    skipped: Vec<String>,
    message: String,
}

// ============================================================
// apply
// ============================================================
fn apply(flags: &GlobalFlags) -> Result<(), String> {
    let moddir = mod_dir();

    let id = match read_boot_id(&moddir)? {
        Some(id) => id,
        None => {
            let msg = match detect_lang() {
                Lang::Zh => "bootanimation.txt 为空，跳过".into(),
                Lang::En => "bootanimation.txt is empty, skipping".into(),
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
    };

    let zip_path = animation_zip(&moddir, &id);
    if !zip_path.exists() {
        let msg = format!(
            "{}: {}",
            match detect_lang() {
                Lang::Zh => "动画不存在",
                Lang::En => "animation not found",
            },
            zip_path.display()
        );
        log_error(flags, &msg);
        if flags.json {
            json_output(&CmdResult {
                success: false,
                data: Some(ApplyData {
                    animation_id: Some(id),
                    message: msg.clone(),
                }),
                error: Some(msg),
            })?;
        }
        return Ok(());
    }

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

    if is_mounted(&sys)? {
        let msg = format!(
            "{}: {}",
            match detect_lang() {
                Lang::Zh => "已挂载，跳过",
                Lang::En => "already mounted, skipping",
            },
            sys
        );
        log_info(flags, &msg);
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
        return Ok(());
    }

    let zip_str = zip_path.to_string_lossy();
    mount(
        Some(Path::new(&*zip_str)),
        Path::new(&sys),
        None,
        MountFlags::BIND,
        None,
    )
    .map_err(|e| format!("mount 失败 {} -> {}: {:?}", zip_str, sys, e))?;

    let ok_main = match detect_lang() {
        Lang::Zh => format!("已挂载: {} -> {}", zip_str, sys),
        Lang::En => format!("mounted: {} -> {}", zip_str, sys),
    };
    log_ok(flags, &ok_main);

    let mut dark_done = false;
    if let Some(dark) = dark_animation_path(&sys) {
        if let Ok(()) = mount(
            Some(Path::new(&*zip_str)),
            Path::new(&dark),
            None,
            MountFlags::BIND,
            None,
        ) {
            let ok_dark = match detect_lang() {
                Lang::Zh => format!("已挂载 dark: {} -> {}", zip_str, dark),
                Lang::En => format!("mounted dark: {} -> {}", zip_str, dark),
            };
            log_ok(flags, &ok_dark);
            dark_done = true;
        }
    }

    let msg = format!(
        "{}: {}{}",
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
// apply_temp
// ============================================================
fn apply_temp(flags: &GlobalFlags, id: &str) -> Result<(), String> {
    let moddir = mod_dir();
    let zip_path = animation_zip(&moddir, id);

    if !zip_path.exists() {
        let msg = format!(
            "{}: {}",
            match detect_lang() {
                Lang::Zh => "动画不存在",
                Lang::En => "animation not found",
            },
            zip_path.display()
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

    if is_mounted(&sys)? {
        let msg = format!(
            "{}: {}",
            match detect_lang() {
                Lang::Zh => "系统动画已被挂载，请先卸载",
                Lang::En => "system animation already mounted, revert first",
            },
            sys
        );
        log_error(flags, &msg);
        if flags.json {
            json_output(&CmdResult {
                success: false,
                data: Some(ApplyTempData {
                    animation_id: id.to_string(),
                    message: msg.clone(),
                    main_mounted: false,
                    dark_mounted: false,
                }),
                error: Some(msg),
            })?;
        }
        return Ok(());
    }

    let zip_str = zip_path.to_string_lossy();
    mount(
        Some(Path::new(&*zip_str)),
        Path::new(&sys),
        None,
        MountFlags::BIND,
        None,
    )
    .map_err(|e| format!("mount 失败 {} -> {}: {:?}", zip_str, sys, e))?;

    let ok_main = match detect_lang() {
        Lang::Zh => format!("已临时挂载: {} -> {}", zip_str, sys),
        Lang::En => format!("temporarily mounted: {} -> {}", zip_str, sys),
    };
    log_ok(flags, &ok_main);

    let mut dark_mounted = false;
    if let Some(dark) = dark_animation_path(&sys) {
        if let Ok(()) = mount(
            Some(Path::new(&*zip_str)),
            Path::new(&dark),
            None,
            MountFlags::BIND,
            None,
        ) {
            let ok_dark = match detect_lang() {
                Lang::Zh => format!("已临时挂载 dark: {} -> {}", zip_str, dark),
                Lang::En => format!("temporarily mounted dark: {} -> {}", zip_str, dark),
            };
            log_ok(flags, &ok_dark);
            dark_mounted = true;
        }
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
        umount(Path::new(&sys))
            .map_err(|e| format!("umount 失败 {}: {:?}", sys, e))?;
        let ok_msg = match detect_lang() {
            Lang::Zh => format!("已卸载: {}", sys),
            Lang::En => format!("unmounted: {}", sys),
        };
        log_ok(flags, &ok_msg);
        unmounted.push(sys.clone());
    }

    if let Some(dark) = dark_animation_path(&sys) {
        if is_mounted(&dark)? {
            let _ = umount(Path::new(&dark));
            let ok_msg = match detect_lang() {
                Lang::Zh => format!("已卸载: {}", dark),
                Lang::En => format!("unmounted: {}", dark),
            };
            log_ok(flags, &ok_msg);
            unmounted.push(dark);
        }
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
    let sys = detect_system_animation();
    let moddir = mod_dir();
    let id = read_boot_id(&moddir).unwrap_or(None);

    let module_exists = id
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
        animation_id: id.clone(),
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
        let l_id = match detect_lang() {
            Lang::Zh => "配置动画 ID",
            Lang::En => "animation_id",
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
        let sys_str = sys.as_deref().unwrap_or(match detect_lang() {
            Lang::Zh => "(未检测到)",
            Lang::En => "(not found)",
        });
        let id_str = id.as_deref().unwrap_or(match detect_lang() {
            Lang::Zh => "(未设置)",
            Lang::En => "(not set)",
        });
        let sources_str = if !main_sources.is_empty() {
            format!(" ({})", main_sources.join(", "))
        } else {
            String::new()
        };
        eprintln!("{}: {}", l_sys, sys_str);
        eprintln!("{}: {}", l_id, id_str);
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

    let id = match read_boot_id(&moddir)? {
        Some(id) => id,
        None => {
            let msg = match detect_lang() {
                Lang::Zh => "bootanimation.txt 为空",
                Lang::En => "bootanimation.txt is empty",
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
        match detect_lang() {
            Lang::Zh => "哈希不一致（可能被覆盖）".into(),
            Lang::En => "hash mismatch (may be overridden)".into(),
        }
    };

    let data = CheckData {
        animation_id: Some(id.clone()),
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
            Lang::Zh => "配置动画 ID",
            Lang::En => "animation_id",
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
        let mod_h = module_hash
            .as_ref()
            .map(|h| h[..16].to_string())
            .unwrap_or_else(|| match detect_lang() {
                Lang::Zh => "(不存在)".into(),
                Lang::En => "(not found)".into(),
            });
        let sys_h = system_hash
            .as_ref()
            .map(|h| h[..16].to_string())
            .unwrap_or_else(|| match detect_lang() {
                Lang::Zh => "(不存在)".into(),
                Lang::En => "(not found)".into(),
            });
        eprintln!("{}: {}", l_id, id);
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
// use
// ============================================================
fn use_cmd(flags: &GlobalFlags, id: &str) -> Result<(), String> {
    let moddir = mod_dir();
    let zip_path = animation_zip(&moddir, id);

    if !zip_path.exists() {
        let msg = format!(
            "{}: {}",
            match detect_lang() {
                Lang::Zh => "动画不存在",
                Lang::En => "animation not found",
            },
            zip_path.display()
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

    let txt_path = moddir.join(BOOT_TXT);
    fs::write(&txt_path, id)
        .map_err(|e| format!("写入 {} 失败: {}", txt_path.display(), e))?;

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

fn generate_desc(config: &Config, w: u32, h: u32) -> String {
    let mut d = format!("{} {} {}\n", w, h, config.fps.unwrap_or(60));
    for p in &config.part {
        d.push_str(&format!(
            "{} {} {} {}\n",
            p.part_type.as_deref().unwrap_or("p"),
            p.loop_count.unwrap_or(0),
            p.pause.unwrap_or(0),
            p.path
        ));
    }
    d
}

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
            add_dir_to_zip(zip, &p, &format!("{}/", zp), opts)?;
        } else {
            zip.start_file(&zp, opts).map_err(|e| e.to_string())?;
            let data = fs::read(&p).map_err(|e| e.to_string())?;
            zip.write_all(&data).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn make_zip(src: &Path, config: &Config, out: &Path) -> Result<(), String> {
    let (w, h) = if config.width.is_some() && config.height.is_some() {
        (config.width.unwrap(), config.height.unwrap())
    } else {
        detect_resolution()
    };

    let desc = generate_desc(config, w, h);
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

    let current_id = read_boot_id(&moddir).unwrap_or(None);

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
            &fs::read_to_string(&info_path).map_err(|e| e.to_string())?
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
            &fs::read_to_string(&cfg_path).map_err(|e| e.to_string())?
        )
        .map_err(|e| format!("{} config.json {}", info.id, e))?;

        let target = moddir.join(ANIM_DIR).join(&info.id);
        let is_update = target.exists();

        let mut was_active = false;
        if is_update {
            if let Some(ref cur) = current_id {
                if cur == &info.id {
                    was_active = true;
                    if let Some(sys) = detect_system_animation() {
                        if is_mounted(&sys).unwrap_or(false) {
                            let _ = umount(Path::new(&sys));
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
                                let _ = umount(Path::new(&dark));
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
        make_zip(&path, &config, &out_zip)?;

        fs::copy(&info_path, target.join("info.json")).map_err(|e| e.to_string())?;

        if was_active {
            let zip_str = out_zip.to_string_lossy();
            if let Some(sys) = detect_system_animation() {
                if !is_mounted(&sys).unwrap_or(false) {
                    let _ = mount(
                        Some(Path::new(&*zip_str)),
                        Path::new(&sys),
                        None,
                        MountFlags::BIND,
                        None,
                    );
                    let ok = format!(
                        "{}: {} -> {}",
                        match detect_lang() {
                            Lang::Zh => "已重新挂载",
                            Lang::En => "remounted",
                        },
                        zip_str,
                        sys
                    );
                    log_ok(flags, &ok);

                    if let Some(dark) = dark_animation_path(&sys) {
                        let _ = mount(
                            Some(Path::new(&*zip_str)),
                            Path::new(&dark),
                            None,
                            MountFlags::BIND,
                            None,
                        );
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

    if !installed.is_empty() && updated.is_empty() {
        let txt = moddir.join(BOOT_TXT);
        let empty = fs::read_to_string(&txt)
            .map(|s| s.trim().is_empty())
            .unwrap_or(true);
        if empty {
            fs::write(&txt, &installed[0]).map_err(|e| e.to_string())?;
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
        "{} {}, {} {}, {} {}",
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
        skipped.len()
    );
    log_info(flags, &msg);

    if flags.json {
        json_output(&CmdResult {
            success: true,
            data: Some(InstallData {
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
  install <zip>              {install_zh}
  help                       {help_zh}

{opt}:
  --json                     {json}
  --logs                     {logs}

{all}
"#,
        help = h,
        prog = prog,
        cmd = c,
        opt = o,
        apply_zh = match detect_lang() {
            Lang::Zh => "挂载 bootanimation.txt 中指定的动画",
            Lang::En => "mount animation specified in bootanimation.txt",
        },
        temp_zh = match detect_lang() {
            Lang::Zh => "临时挂载指定动画（不写文件，重启失效）",
            Lang::En => "temporarily mount an animation (no write, lost on reboot)",
        },
        revert_zh = match detect_lang() {
            Lang::Zh => "卸载开机动画",
            Lang::En => "unmount boot animation",
        },
        status_zh = match detect_lang() {
            Lang::Zh => "查看挂载状态",
            Lang::En => "show mount status",
        },
        check_zh = match detect_lang() {
            Lang::Zh => "检查动画是否生效",
            Lang::En => "check if animation is active",
        },
        use_zh = match detect_lang() {
            Lang::Zh => "设置默认动画（写文件，不挂载）",
            Lang::En => "set default animation (write file, no mount)",
        },
        install_zh = match detect_lang() {
            Lang::Zh => "安装动画包",
            Lang::En => "install animation package",
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