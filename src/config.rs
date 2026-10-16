//! `\palyer.toml` —— 加速矩阵配置（指令集档位 × 渲染路径）。
//!
//! 位置：启动卷根目录（与本程序写 `HWBENCH*.TXT` 同层）。
//! 解析：`toml` 1.x（no_std）+ serde。缺失/非法 → 全默认 + 屏显一行，不 halt。
//!
//! ```toml
//! version = 1
//! [accel]
//! level = "auto"
//! [render]
//! mode   = "auto"
//! blt_op = "buffer_to_video"
//! [render.tune]
//! frames = 30
//! ```

use alloc::format;
use alloc::string::String;
use core::sync::atomic::{AtomicU8, AtomicU32, Ordering};

use serde::Deserialize;
use uefi::cstr16;

use crate::error::NyaStatus;
use crate::fs::Fs;

/// 配置文件名（启动卷根目录）。
pub const CONFIG_PATH: &str = "palyer.toml";

/// 指令集加速档位。`Auto` = 运行时探 CPUID/XCR0 取最高可用。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AccelLevel {
    #[default]
    Auto,
    Scalar,
    Sse2,
    Ssse3,
    Sse41,
    Sse42,
    Avx,
    Avx2,
    Avx512,
}

impl AccelLevel {
    fn as_u8(self) -> u8 {
        match self {
            Self::Auto => 0,
            Self::Scalar => 1,
            Self::Sse2 => 2,
            Self::Ssse3 => 3,
            Self::Sse41 => 4,
            Self::Sse42 => 5,
            Self::Avx => 6,
            Self::Avx2 => 7,
            Self::Avx512 => 8,
        }
    }
    fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Scalar,
            2 => Self::Sse2,
            3 => Self::Ssse3,
            4 => Self::Sse41,
            5 => Self::Sse42,
            6 => Self::Avx,
            7 => Self::Avx2,
            8 => Self::Avx512,
            _ => Self::Auto,
        }
    }
}

/// 渲染路径。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum RenderMode {
    /// 开机微基准在 direct 与 blt 之间选快的。
    #[default]
    Auto,
    /// CPU 直写 framebuffer。
    Direct,
    /// 走 GOP Blt 引擎。
    Blt,
}

impl RenderMode {
    fn as_u8(self) -> u8 {
        match self {
            Self::Auto => 0,
            Self::Direct => 1,
            Self::Blt => 2,
        }
    }
    fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Direct,
            2 => Self::Blt,
            _ => Self::Auto,
        }
    }
}

/// `mode = "blt"` 时用哪个 Blt 动作。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum BltOpKind {
    #[default]
    BufferToVideo,
    VideoToVideo,
    VideoFill,
    VideoToBltBuffer,
}

#[derive(Debug, Clone, Copy, Deserialize, Default)]
#[serde(default)]
pub struct AccelConfig {
    pub level: AccelLevel,
}

const fn default_frames() -> u32 {
    30
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(default)]
pub struct RenderTune {
    #[serde(default = "default_frames")]
    pub frames: u32,
}

impl Default for RenderTune {
    fn default() -> Self {
        Self { frames: default_frames() }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Default)]
#[serde(default)]
pub struct RenderConfig {
    pub mode: RenderMode,
    pub blt_op: BltOpKind,
    pub tune: RenderTune,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(default)]
pub struct Config {
    pub version: u32,
    pub accel: AccelConfig,
    pub render: RenderConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self { version: 1, accel: AccelConfig::default(), render: RenderConfig::default() }
    }
}

/// 解析结果：配置 + 可选的屏显警告（文件不存在不算错，返回 None）。
pub fn parse() -> (Config, Option<String>) {
    let mut fs = match Fs::new() {
        Ok(f) => f,
        Err(e) => return (Config::default(), Some(format!("cfg: open volume failed: {:?}", e))),
    };

    let bytes = match fs.read_file(cstr16!("palyer.toml")) {
        Ok(b) => b,
        // 没有配置文件是正常情况：静默用默认。
        Err(NyaStatus::Uefi(err)) if err.status() == uefi::Status::NOT_FOUND => {
            return (Config::default(), None)
        }
        Err(e) => return (Config::default(), Some(format!("cfg: read failed: {:?}", e))),
    };

    let text = String::from_utf8_lossy(&bytes);
    match toml::from_str::<Config>(&text) {
        Ok(cfg) => (cfg, None),
        Err(e) => (
            Config::default(),
            Some(format!("cfg: parse error: {} -- using defaults", e)),
        ),
    }
}

// ---------- 生效值（原子存储；AP/任意处可读，无需 OnceLock） ----------

static ACCEL: AtomicU8 = AtomicU8::new(0);
static RENDER: AtomicU8 = AtomicU8::new(0);
static BLTOP: AtomicU8 = AtomicU8::new(0);
static TUNE_FRAMES: AtomicU32 = AtomicU32::new(default_frames());
/// 0 = 未测，1 = direct，2 = blt（`auto` 的微基准结论）
static RESOLVED_RENDER: AtomicU8 = AtomicU8::new(0);

/// 开机读一次；返回需要屏显的警告行。
pub fn load() -> Option<String> {
    let (cfg, warn) = parse();
    ACCEL.store(cfg.accel.level.as_u8(), Ordering::Relaxed);
    RENDER.store(cfg.render.mode.as_u8(), Ordering::Relaxed);
    BLTOP.store(cfg.render.blt_op as u8, Ordering::Relaxed);
    TUNE_FRAMES.store(cfg.render.tune.frames.max(1), Ordering::Relaxed);
    log::info!(
        "cfg: version={} accel.level={:?} render.mode={:?} blt_op={:?} tune.frames={}",
        cfg.version,
        cfg.accel.level,
        cfg.render.mode,
        cfg.render.blt_op,
        cfg.render.tune.frames
    );
    warn
}

pub fn accel_level() -> AccelLevel {
    AccelLevel::from_u8(ACCEL.load(Ordering::Relaxed))
}

pub fn render_mode() -> RenderMode {
    RenderMode::from_u8(RENDER.load(Ordering::Relaxed))
}

pub fn blt_op() -> BltOpKind {
    match BLTOP.load(Ordering::Relaxed) {
        1 => BltOpKind::VideoToVideo,
        2 => BltOpKind::VideoFill,
        3 => BltOpKind::VideoToBltBuffer,
        _ => BltOpKind::BufferToVideo,
    }
}

pub fn tune_frames() -> u32 {
    TUNE_FRAMES.load(Ordering::Relaxed)
}

/// 开机微基准测完把结论写进来。
pub fn set_resolved_render(m: RenderMode) {
    RESOLVED_RENDER.store(m.as_u8(), Ordering::Relaxed);
}

/// 实际生效的渲染路径：`auto` 用微基准结果（未测则退回 direct）。
pub fn effective_render() -> RenderMode {
    match render_mode() {
        RenderMode::Auto => match RESOLVED_RENDER.load(Ordering::Relaxed) {
            2 => RenderMode::Blt,
            _ => RenderMode::Direct,
        },
        explicit => explicit,
    }
}
