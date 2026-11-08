use crate::{archive::Archive, storage::child_file};
use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Definition {
    pub id: String,
    pub label: String,
    pub goods_id: String,
    pub item_id: String,
    pub ssu_id: String,
    pub manifest: String,
    pub extras: Option<String>,
    pub asset_root: String,
    pub adapter: String,
    pub baseline_verified: bool,
}

pub struct Vehicle {
    pub archive: Archive,
    pub config: Value,
    pub cart_template: Value,
    pub cart: Mutex<Value>,
    pub materials: HashMap<String, Value>,
    pub items: HashMap<String, Value>,
}

pub struct Registry {
    root: PathBuf,
    definitions: Vec<Definition>,
    loaded: Mutex<HashMap<String, Arc<Vehicle>>>,
}

impl Registry {
    pub fn load(root: &Path) -> Result<Self> {
        let data: Value =
            serde_json::from_slice(&std::fs::read(root.join("config/vehicles.json"))?)?;
        ensure!(data["schemaVersion"] == 1, "不支持的车型注册表版本");
        let definitions: Vec<Definition> = serde_json::from_value(data["vehicles"].clone())?;
        ensure!(!definitions.is_empty(), "车型注册表为空");
        let mut ids = std::collections::HashSet::new();
        for d in &definitions {
            ensure!(ids.insert(d.id.clone()), "重复的车型标识");
        }
        Ok(Self {
            root: root.into(),
            definitions,
            loaded: Default::default(),
        })
    }
    pub fn list(&self) -> Value {
        let rows:Vec<_>=self.definitions.iter().map(|d|{
            let acceptance=self.root.join("vehicles").join(&d.id).join("acceptance.json");
            let verified=std::fs::read(&acceptance).ok().and_then(|s|serde_json::from_slice::<Value>(&s).ok()).is_some_and(|v|v["complete"]==true)||d.baseline_verified;
            let captured=self.root.join(&d.manifest).is_file();
            json!({"id":d.id,"label":d.label,"goodsId":d.goods_id,"itemId":d.item_id,"ssuId":d.ssu_id,"source":format!("https://www.xiaomiev.com/configurator/select?goodsId={}&itemId={}&ssuId={}&type=3d",d.goods_id,d.item_id,d.ssu_id),"captured":captured,"mapped":self.root.join("vehicles").join(&d.id).join("adapter.json").exists(),"available":verified,"status":if verified{"可编辑 · 基础映射已验证"}else{"适配中 · 完整验收未通过"}})
        }).collect();
        json!({"vehicles":rows})
    }
    pub fn get(&self, id: &str) -> Result<Arc<Vehicle>> {
        let definition = self
            .definitions
            .iter()
            .find(|d| d.id == id)
            .context("未知车型，未替换为其他车型")?;
        if let Some(v) = self.loaded.lock().unwrap().get(id) {
            return Ok(v.clone());
        }
        let adapter: Value = serde_json::from_slice(&std::fs::read(child_file(
            &self.root,
            &definition.adapter,
        )?)?)?;
        ensure!(adapter["schemaVersion"] == 1, "不支持的车型适配数据版本");
        let config = adapter["configuration"].clone();
        ensure!(
            config["data"]["carSsuDetail"]["configurator"]["tabList"].is_array(),
            "车型配置数据缺失"
        );
        let mut items = HashMap::new();
        collect_items(&config, &mut items);
        let cart_template = adapter["cartTemplate"].clone();
        let vehicle = Arc::new(Vehicle {
            archive: Archive::load(&self.root, definition)?,
            config,
            cart: Mutex::new(cart_template["data"]["cart"].clone()),
            cart_template,
            materials: serde_json::from_value(adapter["materialOverrides"].clone())?,
            items,
        });
        self.loaded
            .lock()
            .unwrap()
            .insert(id.into(), vehicle.clone());
        Ok(vehicle)
    }
}

pub fn item_id(v: &Value) -> String {
    v.as_str()
        .map(String::from)
        .unwrap_or_else(|| v.to_string())
}
fn collect_items(v: &Value, items: &mut HashMap<String, Value>) {
    match v {
        Value::Object(o) => {
            if o.contains_key("ssuId") && o.contains_key("itemIdentifier") {
                items.insert(item_id(&v["ssuId"]), v.clone());
            }
            for child in o.values() {
                collect_items(child, items);
            }
        }
        Value::Array(a) => {
            for child in a {
                collect_items(child, items);
            }
        }
        _ => {}
    }
}
fn array(v: &Value) -> &[Value] {
    v.as_array().map(Vec::as_slice).unwrap_or(&[])
}

impl Vehicle {
    pub fn option_items(&self) -> Vec<&Value> {
        array(&self.config["data"]["carSsuDetail"]["configurator"]["tabList"])
            .iter()
            .filter(|t| t["tabName"] == "选装")
            .flat_map(|t| array(&t["groupList"]))
            .flat_map(|g| array(&g["subGroupList"]))
            .flat_map(|g| array(&g["itemList"]))
            .collect()
    }
    pub fn catalogue(&self, id: &str) -> Value {
        let tabs:Vec<_>=array(&self.config["data"]["carSsuDetail"]["configurator"]["tabList"]).iter().filter(|t|t["tabName"]!="选装").map(|t|{
            let groups:Vec<_>=array(&t["groupList"]).iter().filter(|g|g["groupName"].as_str().is_some_and(|s|!s.is_empty())).map(|g|{
                let items:Vec<_>=array(&g["subGroupList"]).iter().flat_map(|s|array(&s["itemList"])).filter(|i|i["visibleStatus"].as_i64().unwrap_or(1)==1).map(|i|json!({"id":i["ssuId"],"name":i["name"],"identity":i["itemIdentifier"],"image":format!("/__config-image/{}?vehicle={id}",item_id(&i["ssuId"])),"default":i["defaultSelect"].as_bool().unwrap_or(false),"toggle":i["operateType"]=="MULTIPLE_CHOICE"&&i["cancelFlag"]==true})).collect();
                json!({"name":g["groupName"],"items":items})
            }).collect();json!({"name":t["tabName"],"groups":groups})
        }).collect();
        let options = self.option_items();
        let defaults:Vec<_>=options.iter().map(|i|json!({"params":json!({"nr_car_sku_id":i["ssuId"],"nr_tab_type":"选装","nr_car_group_tag":"默认选装","nr_car_sku_type":i["itemIdentifier"],"nr_car_sku_name":i["name"]}).to_string(),"label":i["name"]})).collect();
        json!({"tabs":tabs,"options":options.iter().map(|i|i["ssuId"].clone()).collect::<Vec<_>>(),"defaults":defaults})
    }
    pub fn add_cart(&self, post: Option<&str>) -> Result<Value> {
        let request: Value = serde_json::from_str(post.unwrap_or("[]"))?;
        let products = request
            .as_array()
            .and_then(|a| a.iter().find_map(|v| v.get("products")))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let selected: Vec<_> = products
            .iter()
            .filter_map(|p| self.items.get(&item_id(&p["ssuId"])))
            .collect();
        let mut response = self.cart_template.clone();
        let cart = &mut response["data"]["cart"];
        for key in ["presentTotal", "originalTotal", "reduceTotal"] {
            cart["feeDetail"][key] = json!("0");
        }
        cart["feeDetail"]["activityFee"]["activityTotal"] = json!("0");
        if let Some(v) = cart.pointer_mut("/feeDetail/activityFee/activityFeeItems/0/total") {
            *v = json!("0");
        }
        let car = &mut cart["groups"][0]["groups"][0]["productItems"][0];
        let templates = array(&car["properties"]["subProducts"]).to_vec();
        let mut subs = Vec::new();
        for option in selected {
            let id = item_id(&option["ssuId"]);
            let template = templates
                .iter()
                .find(|t| item_id(&t["ssuId"]) == id)
                .or_else(|| {
                    templates.iter().find(|t| {
                        t["extend"]["custom_config_identity_enum"] == option["itemIdentifier"]
                    })
                })
                .or_else(|| templates.first())
                .context("车型选项模板缺失")?;
            let mut sub = template.clone();
            for key in ["ssuId", "skuId", "unitId"] {
                sub[key] = json!(id);
            }
            sub["productName"] = option["name"].clone();
            for key in ["marketPrice", "salePrice", "saleAmount"] {
                sub[key] = json!("0");
            }
            sub["itemId"] = json!(option.get("carItemId").map(item_id).unwrap_or_default());
            if let Some(image) = option["configValueImage"]["mainUrl"].as_str() {
                if ![".mp4", ".webm", ".mov"]
                    .iter()
                    .any(|s| image.to_lowercase().contains(s))
                {
                    sub["img800"] = json!(image);
                }
            }
            sub["extend"]["class_tree_visible"] =
                option.get("visibleStatus").cloned().unwrap_or(json!(1));
            sub["extend"]["custom_config_identity_enum"] = option["itemIdentifier"].clone();
            sub["guidAttributes"] = option.get("guideAttributes").cloned().unwrap_or(json!([]));
            if option["itemIdentifier"] == "outerKit" {
                for (k, v) in [
                    ("class_tab_name", json!("车身")),
                    ("class_tab", json!(7418)),
                    ("class_tree_id", json!(7452)),
                    ("class_tree_name", json!("外观套件")),
                ] {
                    sub["extend"][k] = v;
                }
            }
            subs.push(sub);
        }
        car["properties"]["subProducts"] = json!(subs);
        *self.cart.lock().unwrap() = cart.clone();
        response["data"]["count"] = json!(products.len());
        response["data"]["result"]=json!(products.iter().map(|p|json!({"result":true,"code":0,"unitId":item_id(&p["ssuId"]),"ssuId":p["ssuId"]})).collect::<Vec<_>>());
        Ok(response)
    }
}
