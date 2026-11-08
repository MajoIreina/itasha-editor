use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    sync::Mutex,
};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub multithreading: bool,
    pub threads: usize,
    pub gpu_acceleration: bool,
}

pub struct Performance {
    value: Mutex<Settings>,
    path: PathBuf,
    cpu: usize,
    gpu: bool,
}

impl Performance {
    pub fn load(data: &Path, gpu: bool) -> Result<Self> {
        let cpu = std::thread::available_parallelism()
            .map(usize::from)
            .unwrap_or(1);
        let path = data.join("performance-settings.json");
        let value = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str::<Settings>(s.trim_start_matches('\u{feff}')).ok())
            .filter(|s| s.threads >= 1 && s.threads <= cpu)
            .unwrap_or(Settings {
                multithreading: false,
                threads: cpu.min(4),
                gpu_acceleration: true,
            });
        Ok(Self {
            value: Mutex::new(value),
            path,
            cpu,
            gpu,
        })
    }
    pub fn workers(&self) -> usize {
        let s = self.value.lock().unwrap();
        if s.multithreading { s.threads } else { 1 }
    }
    pub fn state(&self) -> Value {
        let s = self.value.lock().unwrap();
        json!({"multithreading":s.multithreading,"threads":s.threads,"gpuAcceleration":s.gpu_acceleration,"logicalProcessors":self.cpu,"effectiveWorkers":if s.multithreading{s.threads}else{1},"gpuAtStartup":self.gpu,"restartRequired":s.gpu_acceleration!=self.gpu,"parallelTasks":["image-decode"],"backend":"rust"})
    }
    pub fn update(&self, value: &Value) -> Result<Value> {
        let next: Settings = serde_json::from_value(value.clone())
            .map_err(|_| anyhow::anyhow!("性能开关必须为布尔值，线程数必须为正整数"))?;
        ensure!(
            next.threads >= 1 && next.threads <= self.cpu,
            "线程数必须是 1–{} 的正整数",
            self.cpu
        );
        {
            let mut current = self.value.lock().unwrap();
            super::storage::atomic_write(&self.path, &serde_json::to_vec_pretty(&next)?)?;
            *current = next;
        }
        Ok(self.state())
    }
}
