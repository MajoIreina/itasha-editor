//! Offline resource replay. No socket or request object enters this module.
use crate::{AppState, archive::url_path, patches, vehicles::Vehicle};
use anyhow::Result;
use serde_json::{Value, json};
pub struct Resource {
    pub status: u16,
    pub mime: String,
    pub data: Vec<u8>,
}
impl Resource {
    fn bytes(data: Vec<u8>, mime: &str) -> Self {
        Self {
            status: 200,
            mime: mime.into(),
            data,
        }
    }
    fn text(text: &str, status: u16) -> Self {
        Self {
            status,
            mime: "text/plain; charset=utf-8".into(),
            data: text.as_bytes().to_vec(),
        }
    }
    fn json(value: Value) -> Self {
        Self::bytes(
            value.to_string().into_bytes(),
            "application/json; charset=utf-8",
        )
    }
}
pub fn load(
    state: &AppState,
    vehicle: &Vehicle,
    url: &str,
    method: &str,
    body: Option<&str>,
) -> Result<Resource> {
    let path = url_path(url);
    if ["tracking.miui", "sentry.d.xiaomi", "onetrack"]
        .iter()
        .any(|s| url.contains(s))
    {
        return Ok(Resource::text("", 204));
    }
    if path.ends_with("/configurator/icons/closeDoor.svg") {
        return Ok(Resource::bytes(br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M6 20V5l12-2v17H6Zm2-8h3" fill="none" stroke="white" stroke-width="1.5"/></svg>"#.to_vec(),"image/svg+xml"));
    }
    if path.ends_with("/car-configurator/get") {
        let mut config = vehicle.config.clone();
        patches::keep_exterior(&mut config);
        return Ok(Resource::json(config));
    }
    if path.ends_with("/car-configurator/get-tip-info") {
        return Ok(Resource::json(
            json!({"code":0,"data":{"tipInfo":"此选项与当前配置不兼容，请先调整车漆或内饰。"}}),
        ));
    }
    if path.ends_with("/cart/list") {
        return Ok(Resource::json(
            json!({"code":0,"data":vehicle.cart.lock().unwrap().clone()}),
        ));
    }
    if path.ends_with("/cart/add") {
        return Ok(Resource::json(vehicle.add_cart(body)?));
    }
    if path.ends_with("/product/shareEntrance") {
        return Ok(Resource::json(json!({"code":0,"data":{"mode":0}})));
    }
    if let Some(entry) = vehicle.archive.lookup(url, method, body) {
        let mime = entry
            .headers
            .get("content-type")
            .map(String::as_str)
            .unwrap_or("application/octet-stream");
        let data = if path.ends_with("/materialV2") {
            match vehicle.materials.get(&entry.file) {
                Some(material) => serde_json::to_vec(material)?,
                None => vehicle.archive.read(entry)?,
            }
        } else {
            vehicle.archive.read(entry)?
        };
        return Ok(Resource::bytes(patches::apply(data, mime, true), mime));
    }
    use std::io::Write;
    if let Ok(mut log) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(state.data.join("logs/missing-assets.jsonl"))
    {
        let _ = writeln!(log, "{}", json!({"url":url,"method":method,"body":body}));
    }
    Ok(Resource::text("Local archive resource not found", 404))
}
