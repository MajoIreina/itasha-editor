//! HTTP transport and authentication only. Business operations live in api.rs.
use crate::{AppState, api, storage::child_file, vehicles::Vehicle};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::io::Read;
use tiny_http::{Header, Request, Response};

const MAX_REQUEST: u64 = 180 * 1024 * 1024;
struct Reply {
    status: u16,
    mime: String,
    data: Vec<u8>,
    headers: Vec<(String, String)>,
}
impl Reply {
    fn bytes(data: Vec<u8>, mime: &str) -> Self {
        Self {
            status: 200,
            mime: mime.into(),
            data,
            headers: Vec::new(),
        }
    }
    fn json(v: Value) -> Self {
        Self::bytes(
            v.to_string().into_bytes(),
            "application/json; charset=utf-8",
        )
    }
    fn text(s: &str, status: u16) -> Self {
        let mut r = Self::bytes(s.as_bytes().to_vec(), "text/plain; charset=utf-8");
        r.status = status;
        r
    }
    fn header(mut self, k: &str, v: String) -> Self {
        self.headers.push((k.into(), v));
        self
    }
}
fn header<'a>(r: &'a Request, name: &'static str) -> Option<&'a str> {
    r.headers()
        .iter()
        .find(|h| h.field.equiv(name))
        .map(|h| h.value.as_str())
}
fn read_json(request: &mut Request) -> Result<Value> {
    ensure!(
        request
            .body_length()
            .is_none_or(|n| n as u64 <= MAX_REQUEST),
        "请求大小超限"
    );
    let mut bytes = Vec::new();
    request
        .as_reader()
        .take(MAX_REQUEST + 1)
        .read_to_end(&mut bytes)?;
    ensure!(bytes.len() as u64 <= MAX_REQUEST, "请求大小超限");
    Ok(serde_json::from_slice(&bytes)?)
}
fn authenticated(request: &Request, state: &AppState) -> bool {
    state.token.is_empty()
        || header(request, "X-Itasha-Token") == Some(state.token.as_str())
        || header(request, "Cookie").is_some_and(|v| {
            v.split(';')
                .any(|c| c.trim().strip_prefix("itasha_session=") == Some(state.token.as_str()))
        })
}
fn file(path: &std::path::Path) -> Result<Reply> {
    let mime = mime_guess::from_path(path)
        .first_or_octet_stream()
        .to_string();
    Ok(Reply::bytes(std::fs::read(path)?, &mime))
}

pub fn handle(mut request: Request, state: &AppState) {
    let is_head = request.method().as_str() == "HEAD";
    let result = route(&mut request, state);
    let reply = match result {
        Ok(r) => r,
        Err(e) => Reply {
            status: 400,
            mime: "application/json; charset=utf-8".into(),
            data: json!({"error":e.to_string()}).to_string().into_bytes(),
            headers: Vec::new(),
        },
    };
    if reply.status >= 400 {
        eprintln!(
            "{} {} -> {} {}",
            request.method(),
            request.url().split('?').next().unwrap_or("/"),
            reply.status,
            String::from_utf8_lossy(&reply.data)
                .chars()
                .take(300)
                .collect::<String>()
        );
    }
    let mut headers = vec![
        ("Content-Type".to_owned(), reply.mime),
        ("Cache-Control".into(), "no-store".into()),
        ("Cross-Origin-Resource-Policy".into(), "cross-origin".into()),
        ("Access-Control-Allow-Origin".into(), "*".into()),
    ];
    headers.extend(reply.headers);
    let len = reply.data.len();
    let mut response = Response::from_data(if is_head { Vec::new() } else { reply.data })
        .with_status_code(reply.status);
    if is_head {
        response.add_header(Header::from_bytes("Content-Length", len.to_string()).unwrap());
    }
    for (k, v) in headers {
        if let Ok(h) = Header::from_bytes(k, v) {
            response.add_header(h);
        }
    }
    let _ = request.respond(response);
}
fn route(request: &mut Request, state: &AppState) -> Result<Reply> {
    let url = url::Url::parse(&format!("http://127.0.0.1{}", request.url()))?;
    let path = url.path();
    let method = request.method().as_str().to_owned();
    if path == "/__studio_health" {
        return Ok(Reply::json(
            json!({"backend":"rust","version":env!("CARGO_PKG_VERSION"),"apiVersion":1}),
        ));
    }
    let host = header(request, "Host").unwrap_or("");
    let valid_host = host
        .parse::<std::net::SocketAddr>()
        .is_ok_and(|a| a.ip().is_loopback())
        || host
            .strip_prefix("localhost:")
            .is_some_and(|p| p.parse::<u16>().is_ok());
    ensure!(valid_host, "访问来源无效");
    if let Some((_, token)) = url.query_pairs().find(|(k, _)| k == "session") {
        if !state.token.is_empty() && token == state.token && path == "/" {
            let mut reply = Reply::text("", 302);
            reply.headers.push(("Location".into(), "/".into()));
            reply.headers.push((
                "Set-Cookie".into(),
                format!(
                    "itasha_session={}; HttpOnly; SameSite=Strict; Path=/",
                    state.token
                ),
            ));
            return Ok(reply);
        }
    }
    if !authenticated(request, state) {
        return Ok(Reply::text("Forbidden", 403));
    }
    if method == "OPTIONS" {
        return Ok(Reply::text(
            "",
            if path.starts_with("/__api/") {
                403
            } else {
                204
            },
        )
        .header("Access-Control-Allow-Methods", "GET, POST, OPTIONS".into())
        .header("Access-Control-Allow-Headers", "*".into()));
    }
    if method == "POST" && path.starts_with("/__api/") {
        ensure!(
            header(request, "X-Studio-Request") == Some("1"),
            "访问来源无效"
        );
        ensure!(
            header(request, "Origin").is_none_or(|o| o == format!("http://{host}")),
            "访问来源无效"
        );
        let args = read_json(request)?;
        return Ok(Reply::json(api::dispatch(
            state,
            path.trim_start_matches("/__api/"),
            &args,
        )?));
    }
    if method == "POST" && path == "/__archive" {
        let args = read_json(request)?;
        let id = args["vehicle"].as_str().unwrap_or("su7");
        let vehicle = state.vehicles.get(id)?;
        return replay(
            state,
            &vehicle,
            args["url"].as_str().context("缺少资源地址")?,
            args["method"].as_str().unwrap_or("GET"),
            args["body"].as_str(),
        );
    }
    let id = url
        .query_pairs()
        .find(|(k, _)| k == "vehicle")
        .map(|(_, v)| v.into_owned())
        .unwrap_or_else(|| "su7".into());
    match path {
        "/" | "/studio" => file(&state.root.join("app/index.html")),
        "/sw.js" => Ok(file(&state.root.join("app/replay-sw.js"))?
            .header("Service-Worker-Allowed", "/".into())),
        "/__vehicles" => Ok(Reply::json(state.vehicles.list())),
        "/__catalog" => Ok(Reply::json(state.vehicles.get(&id)?.catalogue(&id))),
        "/__viewer" => Ok(Reply::bytes(
            state.vehicles.get(&id)?.archive.boot(&id)?,
            "text/html; charset=utf-8",
        )),
        "/__custom.js" | "/__custom.css" => file(
            &state
                .root
                .join("resources")
                .join(path.trim_start_matches("/__")),
        ),
        "/__health" => Ok(Reply::text("Itasha Rust offline archive", 200)),
        _ if path.starts_with("/__disabled_finance") => Ok(Reply::bytes(
            b"<!doctype html><html></html>".to_vec(),
            "text/html",
        )),
        _ if path.starts_with("/__studio/") => file(&child_file(
            &state.root.join("app"),
            &urlencoding::decode(path.trim_start_matches("/__studio/"))?,
        )?),
        _ if path.starts_with("/__ui/") => file(&child_file(
            &state.root.join("resources/ui"),
            &urlencoding::decode(path.trim_start_matches("/__ui/"))?,
        )?),
        _ if path.starts_with("/__download/") => {
            let name = urlencoding::decode(path.trim_start_matches("/__download/"))?;
            let target = child_file(&state.data.join("exports"), &name)?;
            Ok(file(&target)?.header(
                "Content-Disposition",
                format!(
                    "attachment; filename*=UTF-8''{}",
                    urlencoding::encode(&name)
                ),
            ))
        }
        _ if path.starts_with("/__config-image/") => {
            let vehicle = state.vehicles.get(&id)?;
            let item = vehicle
                .items
                .get(path.rsplit('/').next().unwrap_or(""))
                .context("找不到车辆选项")?;
            let image = item["configValueImage"]["rectangleIconImg"]
                .as_str()
                .or_else(|| item["configValueImage"]["mainUrl"].as_str())
                .unwrap_or("");
            if image.starts_with("/__ui/") {
                return file(&child_file(
                    &state.root.join("resources/ui"),
                    image.trim_start_matches("/__ui/"),
                )?);
            }
            if let Some(entry) = vehicle.archive.lookup(image, "GET", None) {
                let mime = entry
                    .headers
                    .get("content-type")
                    .map(String::as_str)
                    .unwrap_or("");
                if mime.starts_with("image/") {
                    return Ok(Reply::bytes(vehicle.archive.read(entry)?, mime));
                }
            }
            Ok(Reply::bytes(br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 120 72"><rect width="120" height="72" fill="#414141"/><path d="M15 48h90l-10-15H35L22 42z" fill="#90969d"/></svg>"##.to_vec(),"image/svg+xml"))
        }
        _ => {
            let vehicle = state.vehicles.get(&id)?;
            if method == "POST" {
                let mut body = String::new();
                request
                    .as_reader()
                    .take(MAX_REQUEST)
                    .read_to_string(&mut body)?;
                replay(
                    state,
                    &vehicle,
                    &format!("https://www.xiaomiev.com{}", request.url()),
                    &method,
                    Some(&body),
                )
            } else {
                replay(
                    state,
                    &vehicle,
                    &format!("https://www.xiaomiev.com{}", request.url()),
                    &method,
                    None,
                )
            }
        }
    }
}
fn replay(
    state: &AppState,
    vehicle: &Vehicle,
    url: &str,
    method: &str,
    body: Option<&str>,
) -> Result<Reply> {
    let resource = crate::replay::load(state, vehicle, url, method, body)?;
    Ok(Reply {
        status: resource.status,
        mime: resource.mime,
        data: resource.data,
        headers: Vec::new(),
    })
}
