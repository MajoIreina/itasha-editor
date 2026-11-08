use crate::{
    images::decode_data,
    storage::{atomic_write, safe_name},
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    io::{Cursor, Write},
    path::Path,
};

fn number(v: &Value, name: &str) -> Result<f64> {
    let n = v[name].as_f64().context(format!("缺少 {name}"))?;
    ensure!(n.is_finite() && n > 0.0 && n <= 1e9, "输出尺寸或 DPI 无效");
    Ok(n)
}
struct Raster {
    width: u32,
    height: u32,
    data: Vec<u8>,
    color: png::ColorType,
}
fn read_png(data: &[u8]) -> Result<Raster> {
    let mut decoder = png::Decoder::new(Cursor::new(data));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    decoder.set_limits(png::Limits {
        bytes: 512 * 1024 * 1024,
    });
    let mut reader = decoder.read_info()?;
    ensure!(
        u64::from(reader.info().width) * u64::from(reader.info().height) <= 64_000_000,
        "输出图片像素超限"
    );
    let mut buf = vec![0; reader.output_buffer_size().context("图片过大")?];
    let info = reader.next_frame(&mut buf)?;
    buf.truncate(info.buffer_size());
    Ok(Raster {
        width: info.width,
        height: info.height,
        data: buf,
        color: info.color_type,
    })
}
fn png_dpi(data: &[u8], dpi: f64) -> Result<Vec<u8>> {
    let image = read_png(data)?;
    let mut output = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut output, image.width, image.height);
        encoder.set_color(image.color);
        encoder.set_depth(png::BitDepth::Eight);
        let density = (dpi / 0.0254).round().clamp(1.0, u32::MAX as f64) as u32;
        encoder.set_pixel_dims(Some(png::PixelDimensions {
            xppu: density,
            yppu: density,
            unit: png::Unit::Meter,
        }));
        encoder.write_header()?.write_image_data(&image.data)?;
    }
    Ok(output)
}
fn pdf(data: &[u8], width_mm: f64, height_mm: f64) -> Result<Vec<u8>> {
    let image = read_png(data)?;
    let mut rgb = Vec::with_capacity(image.width as usize * image.height as usize * 3);
    match image.color {
        png::ColorType::Rgb => rgb = image.data,
        png::ColorType::Rgba => {
            for p in image.data.chunks_exact(4) {
                for c in &p[..3] {
                    rgb.push(
                        ((u16::from(*c) * u16::from(p[3]) + 255 * (255 - u16::from(p[3])) + 127)
                            / 255) as u8,
                    );
                }
            }
        }
        png::ColorType::Grayscale => {
            for c in image.data {
                rgb.extend([c, c, c]);
            }
        }
        png::ColorType::GrayscaleAlpha => {
            for p in image.data.chunks_exact(2) {
                let c = ((u16::from(p[0]) * u16::from(p[1]) + 255 * (255 - u16::from(p[1])) + 127)
                    / 255) as u8;
                rgb.extend([c, c, c]);
            }
        }
        _ => anyhow::bail!("不支持的输出颜色模式"),
    }
    let w = width_mm * 72.0 / 25.4;
    let h = height_mm * 72.0 / 25.4;
    let unit = (w.max(h) / 14400.0).ceil().max(1.0);
    let w = w / unit;
    let h = h / unit;
    let mut compressed =
        flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    compressed.write_all(&rgb)?;
    let compressed = compressed.finish()?;
    let commands = format!("q {w:.8} 0 0 {h:.8} 0 0 cm /Im0 Do Q");
    let mut objects=vec![b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {w:.8} {h:.8}] /UserUnit {unit} /Resources << /XObject << /Im0 4 0 R >> >> /Contents 5 0 R >>").into_bytes()];
    let mut stream=format!("<< /Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode /Length {} >>\nstream\n",image.width,image.height,compressed.len()).into_bytes();
    stream.extend(compressed);
    stream.extend(b"\nendstream");
    objects.push(stream);
    objects.push(
        format!(
            "<< /Length {} >>\nstream\n{commands}\nendstream",
            commands.len()
        )
        .into_bytes(),
    );
    let mut out = b"%PDF-1.6\n%\xe2\xe3\xcf\xd3\n".to_vec();
    let mut offsets = vec![0];
    for (i, obj) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend(format!("{} 0 obj\n", i + 1).bytes());
        out.extend(obj);
        out.extend(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len()).bytes());
    for n in offsets.iter().skip(1) {
        out.extend(format!("{n:010} 00000 n \n").bytes());
    }
    out.extend(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            offsets.len()
        )
        .bytes(),
    );
    Ok(out)
}
pub fn export(data: &Path, args: &Value) -> Result<Value> {
    let name = safe_name(args["name"].as_str().context("缺少导出文件名")?, "未命名");
    let ext = Path::new(&name)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    ensure!(
        ["png", "svg", "json", "su7design", "zip", "pdf"].contains(&ext),
        "导出格式无效"
    );
    let bytes = if let Some(files) = args["files"].as_array() {
        ensure!(ext == "zip", "多文件导出必须使用 ZIP");
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for item in files {
            let name = item["name"].as_str().context("缺少 ZIP 文件名")?;
            let parts: Vec<_> = name.split('/').collect();
            ensure!(
                !parts.is_empty()
                    && parts
                        .iter()
                        .all(|p| !p.is_empty() && *p != "." && *p != ".."),
                "ZIP 文件路径无效"
            );
            let name = parts
                .iter()
                .map(|p| safe_name(p, "文件"))
                .collect::<Vec<_>>()
                .join("/");
            zip.start_file(
                name,
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Deflated),
            )?;
            let text = item["data"].as_str().context("缺少文件内容")?;
            let bytes = if item["base64"] == true {
                decode_data(text)?
            } else {
                text.as_bytes().to_vec()
            };
            zip.write_all(&bytes)?;
        }
        zip.finish()?.into_inner()
    } else {
        let text = args["data"].as_str().context("缺少导出内容")?;
        let bytes = if args["base64"] == true {
            decode_data(text)?
        } else {
            text.as_bytes().to_vec()
        };
        if let Some(print) = args.get("print") {
            match ext {
                "pdf" => pdf(
                    &bytes,
                    number(print, "widthMm")?,
                    number(print, "heightMm")?,
                )?,
                "png" => png_dpi(&bytes, number(print, "dpi")?)?,
                _ => bytes,
            }
        } else {
            bytes
        }
    };
    let target = data.join("exports").join(&name);
    atomic_write(&target, &bytes)?;
    Ok(
        json!({"path":target.to_string_lossy(),"url":format!("/__download/{}",urlencoding::encode(&name)),"bytes":bytes.len()}),
    )
}
