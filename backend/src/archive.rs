use crate::{storage::child_file, vehicles::Definition};
use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::Value;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub url: String,
    pub method: String,
    pub post_data: Option<String>,
    pub status: u16,
    pub bytes: u64,
    pub file: String,
    pub headers: HashMap<String, String>,
}
pub struct Archive {
    pub target: String,
    pub asset_root: PathBuf,
    entries: Vec<Entry>,
    exact: HashMap<(String, String), usize>,
    paths: HashMap<(String, String), usize>,
    bodies: HashMap<(String, String, String), usize>,
}

pub fn url_path(url: &str) -> String {
    url::Url::parse(url)
        .map(|u| u.path().into())
        .unwrap_or_else(|_| url.split('?').next().unwrap_or(url).into())
}
pub fn canonical(body: Option<&str>) -> String {
    let body = body.unwrap_or("null");
    serde_json::from_str::<Value>(body)
        .map(|v| v.to_string())
        .unwrap_or_else(|_| body.into())
}

impl Archive {
    pub fn load(root: &Path, definition: &Definition) -> Result<Self> {
        let manifest: Value =
            serde_json::from_slice(&std::fs::read(child_file(root, &definition.manifest)?)?)?;
        let target = manifest["target"]
            .as_str()
            .context("资源清单缺少目标地址")?
            .to_owned();
        let mut entries: Vec<Entry> = serde_json::from_value(manifest["entries"].clone())?;
        if let Some(extras) = &definition.extras {
            if root.join(extras).is_file() {
                let extra: Value =
                    serde_json::from_slice(&std::fs::read(child_file(root, extras)?)?)?;
                entries.extend(serde_json::from_value::<Vec<Entry>>(
                    extra.get("entries").unwrap_or(&extra).clone(),
                )?);
            }
        }
        let mut archive = Self {
            target,
            asset_root: root.join(&definition.asset_root),
            entries,
            exact: Default::default(),
            paths: Default::default(),
            bodies: Default::default(),
        };
        for (i, e) in archive.entries.iter().enumerate() {
            if e.status != 200 || e.bytes == 0 {
                continue;
            }
            let path = url_path(&e.url);
            archive.exact.insert((e.method.clone(), e.url.clone()), i);
            archive.paths.insert((e.method.clone(), path.clone()), i);
            archive.bodies.insert(
                (e.method.clone(), path, canonical(e.post_data.as_deref())),
                i,
            );
        }
        Ok(archive)
    }
    pub fn lookup(&self, url: &str, method: &str, body: Option<&str>) -> Option<&Entry> {
        let path = url_path(url);
        let method = method.to_owned();
        self.bodies
            .get(&(method.clone(), path.clone(), canonical(body)))
            .or_else(|| self.exact.get(&(method.clone(), url.to_owned())))
            .or_else(|| self.paths.get(&(method, path)))
            .map(|i| &self.entries[*i])
    }
    pub fn read(&self, entry: &Entry) -> Result<Vec<u8>> {
        Ok(std::fs::read(child_file(&self.asset_root, &entry.file)?)?)
    }
    pub fn boot(&self, id: &str) -> Result<Vec<u8>> {
        let url = url::Url::parse(&self.target)?;
        let target = format!(
            "{}{}&studioVehicle={id}",
            url.path(),
            url.query().map(|q| format!("?{q}")).unwrap_or_default()
        );
        Ok(format!(r#"<!doctype html><meta charset="utf-8"><title>Itasha Editor</title><style>body{{background:#171717;color:#eee;font:20px system-ui;text-align:center;padding:20vh 20px}}</style><p>正在载入离线车辆模型…</p><script>navigator.serviceWorker.register('/sw.js').then(async()=>{{await navigator.serviceWorker.ready;if(!navigator.serviceWorker.controller)await new Promise(r=>navigator.serviceWorker.addEventListener('controllerchange',r,{{once:true}}));location.replace({});}}).catch(e=>document.querySelector('p').textContent='启动失败：'+e.message);</script>"#,serde_json::to_string(&target)?).into_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn canonical_body_ignores_object_key_order() {
        assert_eq!(
            canonical(Some("{\"b\":2,\"a\":1}")),
            canonical(Some("{\"a\":1,\"b\":2}"))
        );
        assert_eq!(url_path("https://x/a?b=2"), "/a");
    }
}
