//! Native codec adapter. No Python interpreter or HTTP service is required.
use crate::settings::Performance;
use anyhow::{Context, Result, bail, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Condvar, Mutex},
    time::{Duration, Instant},
};

const EXTENSIONS: &[&str] = &[
    "png", "apng", "jpg", "jpeg", "jpe", "jfif", "webp", "bmp", "dib", "gif", "svg", "avif",
    "avifs", "heic", "heif", "hif", "tif", "tiff", "ico", "cur", "icns", "psd", "tga", "dds",
    "jp2", "j2k", "jpf", "jpx", "jxl", "qoi", "pcx", "pbm", "pgm", "ppm", "pnm", "pfm", "sgi",
    "rgb", "rgba", "xbm", "xpm",
];
pub const MAX_BYTES: usize = 48 * 1024 * 1024;
pub const MAX_PIXELS: u64 = 64_000_000;
pub fn decode_data(value: &str) -> Result<Vec<u8>> {
    Ok(STANDARD
        .decode(value.split_once(',').map(|(_, v)| v).unwrap_or(value))
        .context("图片编码无效")?)
}

pub struct ImageService {
    executable: PathBuf,
    profile: PathBuf,
    active: Mutex<usize>,
    changed: Condvar,
}
struct Permit<'a>(&'a ImageService);
impl Drop for Permit<'_> {
    fn drop(&mut self) {
        *self.0.active.lock().unwrap() -= 1;
        self.0.changed.notify_all();
    }
}

impl ImageService {
    pub fn new(root: &Path) -> Self {
        Self {
            executable: root.join("desktop/native/imagemagick/magick.exe"),
            profile: root.join("config/sRGB.icc"),
            active: Mutex::new(0),
            changed: Condvar::new(),
        }
    }
    fn acquire(&self, settings: &Performance) -> Permit<'_> {
        let mut active = self.active.lock().unwrap();
        while *active >= settings.workers() {
            active = self
                .changed
                .wait_timeout(active, Duration::from_millis(100))
                .unwrap()
                .0;
        }
        *active += 1;
        Permit(self)
    }
    fn command(&self) -> Command {
        let mut cmd = Command::new(&self.executable);
        cmd.env("MAGICK_CONFIGURE_PATH", self.executable.parent().unwrap())
            .env("MAGICK_THREAD_LIMIT", "1")
            .env("MAGICK_MEMORY_LIMIT", "512MiB")
            .env("MAGICK_MAP_LIMIT", "1GiB")
            .env("MAGICK_DISK_LIMIT", "1GiB")
            .env("MAGICK_TIME_LIMIT", "45");
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x08000000);
        }
        cmd
    }
    fn run(&self, mut cmd: Command) -> Result<Vec<u8>> {
        // Use files for output to avoid a pipe deadlock on large encoded images.
        let output = tempfile::tempfile()?;
        let error = tempfile::tempfile()?;
        cmd.stdin(Stdio::null())
            .stdout(output.try_clone()?)
            .stderr(error.try_clone()?);
        let mut child = cmd
            .spawn()
            .context("无法启动原生图片解码器，请重新构建桌面运行环境")?;
        let start = Instant::now();
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break status;
            }
            if start.elapsed() > Duration::from_secs(50) {
                let _ = child.kill();
                let _ = child.wait();
                bail!("图片解码超时，请缩小图片后重试");
            }
            std::thread::sleep(Duration::from_millis(15));
        };
        use std::io::{Read, Seek};
        let mut output = output;
        output.rewind()?;
        let mut bytes = Vec::new();
        output.take(1024 * 1024).read_to_end(&mut bytes)?;
        if !status.success() {
            let mut error = error;
            error.rewind()?;
            let mut detail = String::new();
            error.take(2000).read_to_string(&mut detail)?;
            bail!(
                "无法解码此图片，文件可能损坏或缺少合成图：{}",
                detail.chars().take(250).collect::<String>()
            );
        }
        Ok(bytes)
    }
    pub fn decode(&self, settings: &Performance, name: &str, payload: &[u8]) -> Result<Value> {
        let ext = Path::new(name)
            .extension()
            .and_then(|v| v.to_str())
            .unwrap_or("")
            .to_lowercase();
        ensure!(
            EXTENSIONS.contains(&ext.as_str()),
            "不支持此图片格式：.{ext}"
        );
        ensure!(payload.len() <= MAX_BYTES, "单张素材请控制在 48 MB 以内");
        if ext == "svg" {
            return Ok(
                json!({"name":name,"data":format!("data:image/svg+xml;base64,{}",STANDARD.encode(payload)),"note":"SVG 矢量素材"}),
            );
        }
        let _permit = self.acquire(settings);
        let directory = tempfile::tempdir()?;
        let input = directory.path().join(format!("input.{ext}"));
        let output = directory.path().join("decoded.png");
        std::fs::write(&input, payload)?;
        let mut identify = self.command();
        identify
            .args(["identify", "-ping", "-format", "%w %h %m %n\n"])
            .arg(&input);
        let info = self.run(identify)?;
        let info = String::from_utf8_lossy(&info);
        let fields: Vec<_> = info
            .lines()
            .next()
            .unwrap_or("")
            .split_whitespace()
            .collect();
        ensure!(fields.len() >= 4, "无法读取图片尺寸");
        let width: u64 = fields[0].parse()?;
        let height: u64 = fields[1].parse()?;
        ensure!(
            width > 0 && height > 0 && width.checked_mul(height).is_some_and(|p| p <= MAX_PIXELS),
            "图片超过 6400 万像素，请先缩小后导入"
        );
        let frames: usize = fields[3].parse().unwrap_or(1);
        let mut convert = self.command();
        convert
            .arg(format!("{}[0]", input.to_string_lossy()))
            .args(["-auto-orient", "-profile"])
            .arg(&self.profile)
            .args([
                "-colorspace",
                "sRGB",
                "-alpha",
                "on",
                "-depth",
                "8",
                "-define",
                "png:compression-level=2",
            ])
            .arg(format!("PNG32:{}", output.to_string_lossy()));
        self.run(convert)?;
        let encoded = std::fs::read(output)?;
        // Header dimensions are checked again after orientation/conversion.
        let decoder = png::Decoder::new(std::io::Cursor::new(&encoded));
        let reader = decoder.read_info()?;
        ensure!(
            u64::from(reader.info().width) * u64::from(reader.info().height) <= MAX_PIXELS,
            "图片像素过大"
        );
        let note = format!(
            "{}{}{}",
            fields[2],
            if frames > 1 {
                " · 动图 / 多页取首帧"
            } else {
                ""
            },
            if ext == "psd" {
                " · 使用合成图"
            } else {
                ""
            }
        );
        Ok(
            json!({"name":name,"data":format!("data:image/png;base64,{}",STANDARD.encode(encoded)),"note":note}),
        )
    }
}
