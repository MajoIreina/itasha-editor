pub mod api;
pub mod archive;
pub mod export;
pub mod http;
pub mod images;
pub mod patches;
pub mod process;
pub mod replay;
pub mod settings;
pub mod storage;
pub mod vehicles;

use std::{path::PathBuf, sync::Arc};

pub struct AppState {
    pub root: PathBuf,
    pub data: PathBuf,
    pub token: String,
    pub settings: settings::Performance,
    pub vehicles: vehicles::Registry,
    pub images: images::ImageService,
    pub writes: std::sync::Mutex<()>,
}

impl AppState {
    pub fn new(
        root: PathBuf,
        data: PathBuf,
        token: String,
        gpu: bool,
    ) -> anyhow::Result<Arc<Self>> {
        for name in ["projects", "exports", "logs"] {
            std::fs::create_dir_all(data.join(name))?;
        }
        let settings = settings::Performance::load(&data, gpu)?;
        let images = images::ImageService::new(&root);
        let vehicles = vehicles::Registry::load(&root)?;
        Ok(Arc::new(Self {
            root,
            data,
            token,
            settings,
            vehicles,
            images,
            writes: Default::default(),
        }))
    }
}
