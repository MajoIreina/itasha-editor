//! Compatibility with the captured renderer. All engine-specific substitutions
//! live here so the storage, HTTP and vehicle modules never depend on minified JS.
use serde_json::Value;
use std::sync::LazyLock;

const RENDER_SIGNATURE: &str = "this.render=function(a,t){if(void 0!==t";
const RENDER_HOOK: &str = "this.render=function(a,t){if(a&&a.isScene&&t){window.__su7Scenes=window.__su7Scenes||{};var su7rt=window.__su7Scenes[a.uuid]||(window.__su7Scenes[a.uuid]={renderer:this,scene:a,camera:t,Texture:ez,PhysicalMaterial:lq,ShaderMaterial:tG,Mesh:tD,Scene:nM,OrthographicCamera:rs,WebGLRenderTarget:eX,PlaneGeometry:t5,Vector3:eZ,Matrix4:af,Raycaster:cp});su7rt.camera=t;if(window.su7Bridge){var su7camera=window.su7Bridge.frame(su7rt);if(su7camera)t=su7camera;}}if(void 0!==t";

pub fn keep_exterior(value: &mut Value) {
    match value {
        Value::Object(o) => {
            if o.get("View").is_some_and(|v| v == "panoramaView") {
                o.insert("View".into(), Value::String("modelView".into()));
            }
            for v in o.values_mut() {
                keep_exterior(v);
            }
        }
        Value::Array(a) => {
            for v in a {
                keep_exterior(v);
            }
        }
        _ => {}
    }
}

pub fn apply(data: Vec<u8>, mime: &str, archived: bool) -> Vec<u8> {
    if !(mime.starts_with("text/") || mime.contains("javascript") || mime.contains("json")) {
        return data;
    }
    let Ok(mut text) = String::from_utf8(data.clone()) else {
        return data;
    };
    if archived && mime.contains("text/html") {
        static META: LazyLock<regex::Regex> = LazyLock::new(|| {
            regex::Regex::new(r#"(?i)<meta\b[^>]*http-equiv=["']Content-Security-Policy["'][^>]*>"#)
                .unwrap()
        });
        text = META.replace_all(&text, "").into_owned();
        text=text.replace("<head>","<head><link rel=\"stylesheet\" href=\"/__custom.css\"><script src=\"/__custom.js\"></script>");
    }
    if archived {
        text = text
            .replace(
                "Content-Security-Policy",
                "X-Archived-Content-Security-Policy",
            )
            .replace("https://afs.airstarfinance.net", "/__disabled_finance");
    }
    if mime.contains("javascript") {
        if text.contains(RENDER_SIGNATURE) {
            text=text.replacen(RENDER_SIGNATURE,RENDER_HOOK,1)
                .replacen("let{modelType:o}=r;a.reflectorRenderListManager.configure","let{modelType:o}=r;window.su7Bridge&&window.su7Bridge.attachViewer(a);a.reflectorRenderListManager.configure",1)
                .replacen("Md.set(p,n)),n};","Md.set(p,n)),window.su7Bridge&&window.su7Bridge.registerMaterial(l,n),n};",1)
                .replacen("a.addPlugin(new fK({...g}))","(a.addPlugin(new fK({...g})),window.su7Bridge&&(a.getPlugin(fK).enable=!1))",1);
        }
        text = text.replace(
            "className:\" w-full h-full bg-[black] absolute z-[1000]\"",
            "className:\"studio-native-loading\",style:{display:\"none\"}",
        );
    }
    if mime.contains("text/html") && text.contains("__next") {
        text = text.replace(
            "</head>",
            "<script src=\"/__studio/bridge.js\"></script></head>",
        );
    }
    text.into_bytes()
}
