//! Windows native child surface. The renderer and input loop do not run on the WebView thread.
use itasha_contracts::{AppError, AppResult};
use itasha_renderer::{
    assets::{self, Plan, Scene, SceneStats},
    gpu::Gpu,
};
use itasha_surface_map::gesture::{GestureService, GestureStamp};
use serde::{Deserialize, Serialize};
use std::{
    num::NonZeroIsize,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tauri::Emitter;
use windows_sys::Win32::{
    Foundation::POINT,
    Graphics::Gdi::ScreenToClient,
    UI::WindowsAndMessaging::{
        CWP_SKIPINVISIBLE, ChildWindowFromPointEx, GWL_EXSTYLE, GetCursorPos, GetWindowLongPtrW,
        HWND_TOP, IsWindowVisible, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SetWindowLongPtrW,
        SetWindowPos, WS_EX_TRANSPARENT,
    },
};
use winit::{
    application::ApplicationHandler,
    dpi::{PhysicalPosition, PhysicalSize},
    event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy},
    keyboard::{Key, ModifiersState},
    platform::windows::EventLoopBuilderExtWindows,
    raw_window_handle::{HasWindowHandle, RawWindowHandle, Win32WindowHandle},
    window::{Window, WindowId},
};

#[derive(Default, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub generation: u64,
    pub state: String,
    pub message: String,
    pub session_epoch: String,
    pub adapter: String,
    pub backend: String,
    pub software: bool,
    pub stats: Option<SceneStats>,
    pub upload_cpu_ms: f64,
    pub ready_ms: f64,
    pub frames: u64,
    pub render_width: u32,
    pub render_height: u32,
    pub target_fps: u16,
    pub antialiasing: String,
    pub temporal_samples: u32,
    pub temporal_pending: bool,
    pub render_scale_limited: bool,
    pub scene_time_minutes: u16,
    pub skybox: bool,
    pub camera_eye: [f32; 3],
    pub camera_distance: f32,
    pub camera_radius: f32,
    pub last_submit_cpu_ms: f64,
    pub visible: bool,
    pub width: u32,
    pub height: u32,
    pub x: i32,
    pub y: i32,
    /// Native sibling hit-test, independent of GPU submission and requested visibility.
    pub native_frontmost: bool,
    pub native_visible: bool,
    pub surfaces: Vec<itasha_contracts::SurfaceInfo>,
    pub decal_updates: u64,
    pub painted_surfaces: Vec<String>,
    pub control_points: Vec<[f32; 2]>,
    pub selected_layer: Option<String>,
    #[serde(skip)]
    native_hwnd: isize,
}
#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Bounds {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub visible: bool,
    pub light: bool,
    /// True while a WebView gesture (e.g. splitter drag) must keep receiving
    /// mouse messages even under this native child window's rectangle.
    #[serde(default)]
    pub pass_through: bool,
}
enum Message {
    Gesture {
        input: Box<GestureInput>,
        reply: std::sync::mpsc::SyncSender<AppResult<()>>,
    },
    Configure(
        itasha_contracts::PerformanceSettings,
        itasha_contracts::SceneSettings,
    ),
    Editor {
        epoch: String,
        revision: u64,
        layers: Vec<itasha_contracts::Layer>,
    },
    Select {
        epoch: String,
        id: Option<String>,
    },
    Artwork {
        epoch: String,
        revision: u64,
        id: String,
        width: u32,
        height: u32,
        pixels: Vec<u8>,
        reply: std::sync::mpsc::SyncSender<bool>,
    },
    Pointer(PointerInput),
    Probe {
        x: f32,
        y: f32,
        reply: std::sync::mpsc::SyncSender<serde_json::Value>,
    },
    Decals,
    Bounds(Bounds),
    Begin {
        generation: u64,
        epoch: String,
    },
    Loaded {
        generation: u64,
        scene: Box<std::result::Result<Scene, String>>,
        started: Instant,
    },
    Focus,
    Reset,
    Stop,
}
type Publish = Arc<dyn Fn(Status) + Send + Sync>;
struct LoadJob {
    generation: u64,
    plan: Plan,
    started: Instant,
}
pub struct NativeViewport {
    document_stamp: Arc<Mutex<Option<(String, u64)>>>,
    surfaces: Arc<Mutex<SurfaceCache>>,
    decals: Arc<Mutex<std::collections::HashMap<String, DecalUpdate>>>,
    parent: isize,
    proxy: EventLoopProxy<Message>,
    generation: Arc<AtomicU64>,
    status: Arc<Mutex<Status>>,
    jobs: std::sync::mpsc::Sender<LoadJob>,
    cached: Mutex<Option<(String, Plan)>>,
}
#[derive(Default)]
struct SurfaceCache {
    topology: Option<Arc<itasha_surface_map::topology::Topology>>,
    fingerprint: String,
    epoch: String,
    generation: u64,
    masks: std::collections::HashMap<String, Vec<f32>>,
}
struct DecalUpdate {
    reply: std::sync::mpsc::SyncSender<bool>,
    epoch: String,
    generation: u64,
    revision: u64,
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}
impl NativeViewport {
    pub fn new(parent: isize, app: tauri::AppHandle) -> std::result::Result<Self, String> {
        let status = Arc::new(Mutex::new(Status {
            state: "idle".into(),
            message: "等待载入车型".into(),
            ..Default::default()
        }));
        let surfaces = Arc::new(Mutex::new(SurfaceCache::default()));
        let document_stamp = Arc::new(Mutex::new(None));
        let host_stamp = document_stamp.clone();
        let decals = Arc::new(Mutex::new(std::collections::HashMap::new()));
        let host_surfaces = surfaces.clone();
        let host_decals = decals.clone();
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let shared = status.clone();
        std::thread::Builder::new()
            .name("native-viewport".into())
            .spawn(move || {
                let reporter = shared.clone();
                let report_app = app.clone();
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let event_loop = EventLoop::<Message>::with_user_event()
                        .with_any_thread(true)
                        .build()
                        .map_err(|e| e.to_string())?;
                    tx.send(event_loop.create_proxy())
                        .map_err(|e| e.to_string())?;
                    let shortcut_app = app.clone();
                    let status_app = app.clone();
                    let publish: Publish = Arc::new(move |status| {
                        let _ = status_app.emit("viewport-status", status);
                    });
                    let mut host = Host {
                        file_drop: Arc::new({
                            let app = app.clone();
                            move |value| {
                                let _ = app.emit("viewport-file-drop", value);
                            }
                        }),
                        gesture_service: GestureService::default(),
                        preview_event: Arc::new({
                            let app = app.clone();
                            move |value| {
                                let _ = app.emit("viewport-preview", value);
                            }
                        }),
                        edit_gesture: None,
                        edit_epoch: String::new(),
                        edit_revision: 0,
                        edit_layers: vec![],
                        selected: None,
                        edit_event: Arc::new({
                            let app = app.clone();
                            move |value| {
                                let _ = app.emit("viewport-edit", value);
                            }
                        }),
                        selection_event: Arc::new({
                            let app = app.clone();
                            move |value| {
                                let _ = app.emit("viewport-selection", value);
                            }
                        }),
                        document_stamp: host_stamp,
                        surfaces: host_surfaces,
                        decals: host_decals,
                        last_decal_revision: std::collections::HashMap::new(),
                        painted_surfaces: std::collections::BTreeSet::new(),
                        parent,
                        window: None,
                        gpu: None,
                        status: shared,
                        publish,
                        generation: 0,
                        bounds: Bounds {
                            x: 0,
                            y: 0,
                            width: 1,
                            height: 1,
                            visible: false,
                            light: false,
                            pass_through: false,
                        },
                        last_cursor: None,
                        button: None,
                        last_click: Instant::now(),
                        dirty: false,
                        last_frame: Instant::now(),
                        performance: Default::default(),
                        scene_settings: Default::default(),
                        last_publish: Instant::now(),
                        reset_bounds: None,
                        modifiers: ModifiersState::empty(),
                        shortcut: Arc::new(move |shortcut| {
                            let _ = shortcut_app.emit("viewport-shortcut", shortcut);
                        }),
                    };
                    event_loop.run_app(&mut host).map_err(|e| e.to_string())
                }));
                let error = match result {
                    Ok(Ok(())) => None,
                    Ok(Err(e)) => Some(e),
                    Err(_) => Some("原生 GPU 视口发生错误；工程服务仍可使用。".into()),
                };
                if let Some(error) = error
                    && let Ok(mut value) = reporter.lock()
                {
                    value.state = "error".into();
                    value.message = error;
                    value.visible = false;
                    let _ = report_app.emit("viewport-status", value.clone());
                }
            })
            .map_err(|e| e.to_string())?;
        let proxy = rx
            .recv_timeout(Duration::from_secs(5))
            .map_err(|e| e.to_string())?;
        let generation = Arc::new(AtomicU64::new(0));
        let current = generation.clone();
        let loader_proxy = proxy.clone();
        let (jobs, receiver) = std::sync::mpsc::channel::<LoadJob>();
        std::thread::Builder::new()
            .name("model-loader".into())
            .spawn(move || {
                while let Ok(mut job) = receiver.recv() {
                    // One decoder at a time; skip superseded jobs before allocating images.
                    while let Ok(next) = receiver.try_recv() {
                        job = next;
                    }
                    if current.load(Ordering::SeqCst) != job.generation {
                        continue;
                    }
                    let result = assets::load(&job.plan);
                    if current.load(Ordering::SeqCst) == job.generation {
                        let _ = loader_proxy.send_event(Message::Loaded {
                            generation: job.generation,
                            scene: Box::new(result),
                            started: job.started,
                        });
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Self {
            document_stamp,
            surfaces,
            decals,
            parent,
            proxy,
            generation,
            status,
            jobs,
            cached: Mutex::new(None),
        })
    }
    pub fn status(&self) -> AppResult<Status> {
        let mut status = self
            .status
            .lock()
            .map(|v| v.clone())
            .map_err(AppError::io)?;
        // Query current HWND state rather than trusting the last submitted frame.
        (status.native_visible, status.native_frontmost) = native_visibility(
            status.native_hwnd,
            self.parent,
            Bounds {
                x: status.x,
                y: status.y,
                width: status.width,
                height: status.height,
                visible: status.visible,
                light: false,
                pass_through: false,
            },
        );
        Ok(status)
    }
    pub fn bounds(&self, bounds: Bounds) -> AppResult<()> {
        if bounds.width > 8192
            || bounds.height > 8192
            || bounds.x.unsigned_abs() > 32768
            || bounds.y.unsigned_abs() > 32768
        {
            return Err(AppError::new("invalid-input", "视口尺寸超出范围。"));
        }
        self.proxy
            .send_event(Message::Bounds(bounds))
            .map_err(AppError::io)
    }
    pub fn focus(&self) -> AppResult<()> {
        self.proxy.send_event(Message::Focus).map_err(AppError::io)
    }
    pub fn reset(&self) -> AppResult<()> {
        self.proxy.send_event(Message::Reset).map_err(AppError::io)
    }
    pub fn document_changed(
        &self,
        stamp: Option<(String, u64)>,
        layers: Vec<itasha_contracts::Layer>,
    ) {
        let (epoch, revision) = stamp.clone().unwrap_or_default();
        let _ = self.proxy.send_event(Message::Editor {
            epoch,
            revision,
            layers,
        });
        if let Ok(mut value) = self.document_stamp.lock() {
            *value = stamp;
        }
    }
    pub fn mask(&self, epoch: &str, id: &str) -> AppResult<Vec<u8>> {
        let cache = self.surfaces.lock().map_err(AppError::io)?;
        if cache.epoch != epoch {
            return Err(AppError::new("stale-session", "外板会话尚未就绪。"));
        }
        let mask = cache
            .masks
            .get(id)
            .ok_or_else(|| AppError::new("surface-missing", "漆面展开未就绪。"))?;
        Ok(mask.iter().flat_map(|v| v.to_le_bytes()).collect())
    }
    pub fn gesture(
        &self,
        input: GestureInput,
    ) -> AppResult<std::sync::mpsc::Receiver<AppResult<()>>> {
        let (reply, receiver) = std::sync::mpsc::sync_channel(1);
        self.proxy
            .send_event(Message::Gesture {
                input: Box::new(input),
                reply,
            })
            .map_err(AppError::io)?;
        Ok(receiver)
    }
    pub fn prepare_layer(&self, epoch: &str, layer: &mut itasha_contracts::Layer) -> AppResult<()> {
        let cache = self.surfaces.lock().map_err(AppError::io)?;
        if cache.epoch != epoch {
            return Err(AppError::new("stale-session", "展开会话已过期。"));
        }
        let topology = cache
            .topology
            .as_ref()
            .ok_or_else(|| AppError::new("surface-missing", "展开尚未就绪。"))?;
        let p = &mut layer.placement;
        let Some((i, b)) = topology.locate(&p.surface_id, [p.x_mm as f32, p.y_mm as f32].into())
        else {
            return Err(AppError::new(
                "invalid-anchor",
                "素材锚点必须位于有效漆面内。",
            ));
        };
        let t = &topology.triangles[i];
        p.anchor = Some(itasha_contracts::SurfaceAnchor {
            part_id: t.part_id.clone(),
            region_id: t.region_id.clone(),
            source_triangle_id: t.id.stable_key(),
            barycentric: b.to_array(),
            paint_surface_fingerprint: cache.fingerprint.clone(),
        });
        layer.validate(Some("su7"))
    }
    pub fn transport_layer(
        &self,
        epoch: &str,
        layer: &itasha_contracts::Layer,
        surface_id: String,
        point: [f32; 2],
        offset: [f32; 2],
    ) -> AppResult<itasha_contracts::Layer> {
        let cache = self.surfaces.lock().map_err(AppError::io)?;
        if cache.epoch != epoch {
            return Err(AppError::new("stale-session", "展开会话已过期。"));
        }
        let topology = cache
            .topology
            .as_ref()
            .ok_or_else(|| AppError::new("surface-missing", "展开尚未就绪。"))?;
        itasha_renderer::interaction::transport_anchor(
            topology,
            layer,
            &itasha_renderer::interaction::SurfaceEditPoint {
                surface_id,
                point: point.into(),
            },
            offset,
        )
        .ok_or_else(|| AppError::new("invalid-anchor", "目标不在有效漆面内。"))
    }
    pub fn coverage(
        &self,
        epoch: &str,
        layer: &itasha_contracts::Layer,
    ) -> AppResult<Vec<itasha_surface_map::CoverageFragment>> {
        let topology = {
            let cache = self.surfaces.lock().map_err(AppError::io)?;
            if cache.epoch != epoch {
                return Err(AppError::new("stale-session", "展开会话已过期。"));
            }
            cache
                .topology
                .clone()
                .ok_or_else(|| AppError::new("surface-missing", "展开尚未就绪。"))?
        };
        let p = &layer.placement;
        Ok(topology.coverage_at(
            &p.surface_id,
            [p.x_mm as f32, p.y_mm as f32],
            [p.width_mm as f32, p.height_mm as f32],
            (p.rotation_deg as f32).to_radians(),
            p.cross_surface,
        ))
    }
    pub fn select(&self, epoch: String, id: Option<String>) -> AppResult<()> {
        self.proxy
            .send_event(Message::Select { epoch, id })
            .map_err(AppError::io)
    }
    pub fn pointer(&self, input: PointerInput) -> AppResult<()> {
        if !input.x.is_finite()
            || !input.y.is_finite()
            || input.x.abs() > 8192.
            || input.y.abs() > 8192.
            || !["down", "move", "up", "cancel"].contains(&input.kind.as_str())
        {
            return Err(AppError::new("invalid-input", "指针数据无效"));
        }
        self.proxy
            .send_event(Message::Pointer(input))
            .map_err(AppError::io)
    }
    pub fn probe(&self, x: f32, y: f32) -> AppResult<std::sync::mpsc::Receiver<serde_json::Value>> {
        if !x.is_finite() || !y.is_finite() || x.abs() > 8192. || y.abs() > 8192. {
            return Err(AppError::new("invalid-input", "选取坐标无效"));
        }
        let (reply, receiver) = std::sync::mpsc::sync_channel(1);
        self.proxy
            .send_event(Message::Probe { x, y, reply })
            .map_err(AppError::io)?;
        Ok(receiver)
    }
    pub fn artwork(
        &self,
        epoch: String,
        revision: u64,
        id: String,
        width: u32,
        height: u32,
        pixels: Vec<u8>,
    ) -> AppResult<std::sync::mpsc::Receiver<bool>> {
        if width == 0
            || height == 0
            || width > 2048
            || height > 2048
            || pixels.len() != width as usize * height as usize * 4
        {
            return Err(AppError::new("invalid-input", "素材纹理尺寸无效"));
        }
        let (reply, receiver) = std::sync::mpsc::sync_channel(1);
        self.proxy
            .send_event(Message::Artwork {
                epoch,
                revision,
                id,
                width,
                height,
                pixels,
                reply,
            })
            .map_err(AppError::io)?;
        Ok(receiver)
    }
    pub fn decal(
        &self,
        epoch: String,
        id: String,
        revision: u64,
        width: u32,
        height: u32,
        pixels: Vec<u8>,
    ) -> AppResult<std::sync::mpsc::Receiver<bool>> {
        if width == 0
            || height == 0
            || width > 2048
            || height > 2048
            || pixels.len() != width as usize * height as usize * 4
        {
            return Err(AppError::new("invalid-input", "画布预览像素尺寸无效。"));
        }
        let cache = self.surfaces.lock().map_err(AppError::io)?;
        if cache.epoch != epoch || !cache.masks.contains_key(&id) {
            return Err(AppError::new("stale-session", "目标外板会话已变化。"));
        }
        let (reply, receiver) = std::sync::mpsc::sync_channel(1);
        if let Some(old) = self.decals.lock().map_err(AppError::io)?.insert(
            id,
            DecalUpdate {
                reply,
                epoch,
                generation: cache.generation,
                revision,
                width,
                height,
                pixels,
            },
        ) {
            let _ = old.reply.send(false);
        }
        self.proxy
            .send_event(Message::Decals)
            .map_err(AppError::io)?;
        Ok(receiver)
    }
    pub fn configure(
        &self,
        performance: itasha_contracts::PerformanceSettings,
        scene: itasha_contracts::SceneSettings,
    ) -> AppResult<()> {
        self.proxy
            .send_event(Message::Configure(performance, scene))
            .map_err(AppError::io)
    }
    pub fn load(&self, epoch: String, plan: Plan, force: bool) -> AppResult<()> {
        let mut cached = self.cached.lock().map_err(AppError::io)?;
        if !force
            && cached
                .as_ref()
                .is_some_and(|(previous, value)| previous == &epoch && value == &plan)
            && self.status()?.state != "error"
        {
            return Ok(());
        }
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        *cached = Some((epoch.clone(), plan.clone()));
        self.proxy
            .send_event(Message::Begin { generation, epoch })
            .map_err(AppError::io)?;
        self.jobs
            .send(LoadJob {
                generation,
                plan,
                started: Instant::now(),
            })
            .map_err(AppError::io)?;
        Ok(())
    }
}
impl Drop for NativeViewport {
    fn drop(&mut self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        let _ = self.proxy.send_event(Message::Stop);
    }
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PointerInput {
    pub kind: String,
    pub x: f32,
    pub y: f32,
    #[serde(default)]
    pub shift: bool,
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GestureInput {
    pub kind: String,
    pub stamp: GestureStamp,
    pub sequence: u64,
    pub layer: Option<itasha_contracts::Layer>,
}
struct EditGesture {
    stamp: GestureStamp,
    sequence: u64,
    layer: itasha_contracts::Layer,
    value: itasha_contracts::Layer,
    start: [f32; 2],
    start_surface: String,
    start_screen: [f32; 2],
    control_frame: Option<itasha_renderer::interaction::ScreenControlFrame>,
    grab_offset: [f32; 2],
    handle: Option<usize>,
    epoch: String,
    revision: u64,
}
struct Host {
    gesture_service: GestureService<itasha_contracts::Layer>,
    preview_event: Arc<dyn Fn(serde_json::Value) + Send + Sync>,
    file_drop: Arc<dyn Fn(serde_json::Value) + Send + Sync>,
    edit_gesture: Option<EditGesture>,
    edit_epoch: String,
    edit_revision: u64,
    edit_layers: Vec<itasha_contracts::Layer>,
    selected: Option<String>,
    edit_event: Arc<dyn Fn(serde_json::Value) + Send + Sync>,
    selection_event: Arc<dyn Fn(serde_json::Value) + Send + Sync>,
    document_stamp: Arc<Mutex<Option<(String, u64)>>>,
    surfaces: Arc<Mutex<SurfaceCache>>,
    decals: Arc<Mutex<std::collections::HashMap<String, DecalUpdate>>>,
    last_decal_revision: std::collections::HashMap<String, u64>,
    painted_surfaces: std::collections::BTreeSet<String>,
    parent: isize,
    window: Option<Arc<Window>>,
    gpu: Option<Gpu>,
    status: Arc<Mutex<Status>>,
    publish: Publish,
    generation: u64,
    bounds: Bounds,
    last_cursor: Option<(f64, f64)>,
    button: Option<MouseButton>,
    last_click: Instant,
    dirty: bool,
    last_frame: Instant,
    performance: itasha_contracts::PerformanceSettings,
    scene_settings: itasha_contracts::SceneSettings,
    last_publish: Instant,
    reset_bounds: Option<(itasha_renderer::gpu::Vec3, itasha_renderer::gpu::Vec3)>,
    modifiers: ModifiersState,
    shortcut: Arc<dyn Fn(String) + Send + Sync>,
}
impl Host {
    fn gesture_input(&mut self, input: GestureInput) -> AppResult<()> {
        let stamp = &input.stamp;
        if stamp.session_epoch != self.edit_epoch
            || stamp.base_revision != self.edit_revision.to_string()
            || stamp.mapping_version != 2
        {
            return Err(AppError::new("stale-gesture", "编辑会话已过期。"));
        }
        let error = |e| AppError::new("gesture-rejected", format!("编辑手势被拒绝：{e:?}"));
        match input.kind.as_str() {
            "begin" => {
                let layer = input
                    .layer
                    .ok_or_else(|| AppError::new("invalid-input", "缺少图层。"))?;
                if !self.edit_layers.iter().any(|l| *l == layer && !l.locked) {
                    return Err(AppError::new("stale-layer", "图层已改变或已锁定。"));
                }
                self.gesture_service
                    .begin(stamp.clone(), layer)
                    .map_err(error)?;
            }
            "preview" => {
                let mut layer = input
                    .layer
                    .ok_or_else(|| AppError::new("invalid-input", "缺少图层。"))?;
                {
                    let cache = self.surfaces.lock().map_err(AppError::io)?;
                    let topology = cache
                        .topology
                        .as_ref()
                        .ok_or_else(|| AppError::new("surface-missing", "展开尚未就绪。"))?;
                    let p = &mut layer.placement;
                    let Some((i, b)) =
                        topology.locate(&p.surface_id, [p.x_mm as f32, p.y_mm as f32].into())
                    else {
                        return Err(AppError::new("invalid-anchor", "锚点越过可编辑区域。"));
                    };
                    let t = &topology.triangles[i];
                    p.anchor = Some(itasha_contracts::SurfaceAnchor {
                        part_id: t.part_id.clone(),
                        region_id: t.region_id.clone(),
                        source_triangle_id: t.id.stable_key(),
                        barycentric: b.to_array(),
                        paint_surface_fingerprint: cache.fingerprint.clone(),
                    });
                }
                layer.validate(Some("su7"))?;
                if self
                    .gesture_service
                    .current()
                    .is_none_or(|(_, _, l)| l.id != layer.id || l.source != layer.source)
                {
                    return Err(AppError::new("invalid-input", "手势只能修改原图层。"));
                }
                self.gesture_service
                    .preview(stamp, input.sequence, layer.clone())
                    .map_err(error)?;
                if let Some(gpu) = &mut self.gpu {
                    gpu.preview(layer.clone());
                }
                (self.preview_event)(
                    serde_json::json!({"stamp":stamp,"sequence":input.sequence,"layer":layer}),
                );
            }
            "commit" => {
                if let Some(layer) = self
                    .gesture_service
                    .commit(stamp, input.sequence)
                    .map_err(error)?
                {
                    (self.edit_event)(
                        serde_json::json!({"sessionEpoch":stamp.session_epoch,"revision":stamp.base_revision,"layer":layer}),
                    );
                } else {
                    (self.preview_event)(serde_json::json!({"stamp":stamp,"layer":null}));
                }
            }
            "cancel" => {
                self.gesture_service.cancel(stamp).map_err(error)?;
                if let Some(gpu) = &mut self.gpu {
                    gpu.editor(self.edit_layers.clone(), self.selected.clone());
                }
                (self.preview_event)(serde_json::json!({"stamp":stamp,"layer":null}));
            }
            _ => return Err(AppError::new("invalid-input", "未知编辑手势。")),
        }
        self.dirty = true;
        Ok(())
    }
    fn pointer_input(&mut self, input: PointerInput) -> bool {
        let Some(gpu) = &mut self.gpu else {
            return false;
        };
        match input.kind.as_str() {
            "down" => {
                let handle = gpu.handle_at(input.x, input.y);
                let (layer, image_uv) = if handle.is_some() {
                    (gpu.selected_layer(), None)
                } else {
                    gpu.layer_point_at(input.x, input.y)
                        .map_or((None, None), |(layer, uv)| (Some(layer), Some(uv)))
                };
                let already_selected = self.selected == layer.as_ref().map(|l| l.id.to_string());
                self.selected = layer.as_ref().map(|l| l.id.to_string());
                gpu.select(self.selected.clone());
                (self.selection_event)(
                    serde_json::json!({"sessionEpoch":self.edit_epoch,"id":self.selected}),
                );
                self.dirty = true;
                if let Some(layer) = layer {
                    if layer.locked {
                        return true;
                    }
                    let control_frame = handle.and_then(|_| gpu.control_frame());
                    let start = if control_frame.is_some() {
                        Some(itasha_renderer::interaction::SurfaceEditPoint {
                            surface_id: layer.placement.surface_id.clone(),
                            point: [layer.placement.x_mm as f32, layer.placement.y_mm as f32]
                                .into(),
                        })
                    } else {
                        gpu.edit_point(&layer, input.x, input.y, false)
                    };
                    if let Some(start) = start {
                        let grab_offset = image_uv.map_or_else(
                            || itasha_renderer::interaction::local(&layer, start.point),
                            |uv| itasha_renderer::interaction::image_grab_offset(&layer, uv),
                        );
                        let stamp = GestureStamp {
                            session_epoch: self.edit_epoch.clone(),
                            base_revision: self.edit_revision.to_string(),
                            gesture_id: uuid::Uuid::new_v4().to_string(),
                            mapping_version: 2,
                        };
                        if self
                            .gesture_service
                            .begin(stamp.clone(), layer.clone())
                            .is_err()
                        {
                            return true;
                        }
                        gpu.freeze_control_basis(already_selected.then_some(&layer));
                        self.edit_gesture = Some(EditGesture {
                            stamp,
                            sequence: 0,
                            value: layer.clone(),
                            layer,
                            start: start.point.to_array(),
                            start_surface: start.surface_id,
                            start_screen: [input.x, input.y],
                            control_frame,
                            grab_offset: grab_offset.to_array(),
                            handle,
                            epoch: self.edit_epoch.clone(),
                            revision: self.edit_revision,
                        });
                        return true;
                    }
                    return true;
                }
                false
            }
            "move" => {
                let Some(g) = &mut self.edit_gesture else {
                    return false;
                };
                let next = if let (Some(handle), Some(frame)) = (g.handle, g.control_frame) {
                    Some(itasha_renderer::interaction::transform_screen_control(
                        &g.layer,
                        frame,
                        g.start_screen.into(),
                        [input.x, input.y].into(),
                        handle,
                        input.shift,
                    ))
                } else {
                    gpu.edit_point(&g.layer, input.x, input.y, false)
                        .and_then(|current| {
                            if current.surface_id == g.start_surface
                                && current.point.distance_squared(g.start.into()) < 1e-12
                            {
                                return None;
                            }
                            let cache = self.surfaces.lock().ok()?;
                            itasha_renderer::interaction::transport_anchor(
                                cache.topology.as_ref()?,
                                &g.layer,
                                &current,
                                g.grab_offset,
                            )
                        })
                };
                if let Some(next) = next {
                    g.value = gpu.constrain_layer(&g.value, &next);
                    {
                        if let Ok(cache) = self.surfaces.lock()
                            && let Some(topology) = &cache.topology
                        {
                            let p = &mut g.value.placement;
                            if let Some((i, b)) = topology
                                .locate(&p.surface_id, [p.x_mm as f32, p.y_mm as f32].into())
                            {
                                let t = &topology.triangles[i];
                                p.anchor = Some(itasha_contracts::SurfaceAnchor {
                                    part_id: t.part_id.clone(),
                                    region_id: t.region_id.clone(),
                                    source_triangle_id: t.id.stable_key(),
                                    barycentric: b.to_array(),
                                    paint_surface_fingerprint: cache.fingerprint.clone(),
                                });
                            }
                        }
                    }
                    if g.handle.is_none() {
                        gpu.rebase_control_angle(g.value.placement.rotation_deg as f32);
                    }
                    g.sequence += 1;
                    if self
                        .gesture_service
                        .preview(&g.stamp, g.sequence, g.value.clone())
                        .is_ok()
                    {
                        gpu.preview(g.value.clone());
                        (self.preview_event)(
                            serde_json::json!({"stamp":g.stamp,"sequence":g.sequence,"layer":g.value}),
                        );
                    }
                    self.dirty = true;
                }
                true
            }
            "up" => {
                let Some(g) = self.edit_gesture.take() else {
                    return false;
                };
                gpu.freeze_control_basis(None);
                self.dirty = true;
                if let Ok(Some(layer)) = self.gesture_service.commit(&g.stamp, g.sequence) {
                    (self.edit_event)(
                        serde_json::json!({"sessionEpoch":g.epoch,"revision":g.revision.to_string(),"layer":layer}),
                    );
                } else {
                    (self.preview_event)(serde_json::json!({"stamp":g.stamp,"layer":null}));
                }

                true
            }
            "cancel" => {
                let active = self.edit_gesture.take().is_some();
                gpu.freeze_control_basis(None);
                self.gesture_service.invalidate();
                (self.preview_event)(
                    serde_json::json!({"stamp":{"sessionEpoch":self.edit_epoch},"layer":null}),
                );
                gpu.editor(self.edit_layers.clone(), self.selected.clone());
                self.dirty = true;
                active
            }
            _ => false,
        }
    }
    fn report(&mut self) {
        let hwnd = self
            .window
            .as_ref()
            .and_then(|window| window.window_handle().ok())
            .and_then(|handle| match handle.as_raw() {
                RawWindowHandle::Win32(handle) => Some(handle.hwnd.get()),
                _ => None,
            })
            .unwrap_or(0);
        let (native_visible, native_frontmost) = native_visibility(hwnd, self.parent, self.bounds);
        if let Ok(mut status) = self.status.lock() {
            if let Some(gpu) = &self.gpu {
                status.adapter = gpu.adapter.clone();
                status.backend = gpu.backend.clone();
                status.software = gpu.software;
                status.frames = gpu.frames;
                status.last_submit_cpu_ms = gpu.last_submit_ms;
            }
            status.visible = self.bounds.visible;
            status.width = self.bounds.width;
            status.height = self.bounds.height;
            status.x = self.bounds.x;
            status.y = self.bounds.y;
            status.native_hwnd = hwnd;
            status.native_visible = native_visible;
            status.native_frontmost = native_frontmost;
            status.painted_surfaces = self
                .gpu
                .as_ref()
                .map(|g| g.painted_parts())
                .unwrap_or_default();
            status.control_points = self
                .gpu
                .as_ref()
                .map(|g| g.control_points())
                .unwrap_or_default();
            status.selected_layer = self.selected.clone();
            (self.publish)(status.clone());
        }
        self.last_publish = Instant::now();
    }
    fn fail(&mut self, error: String) {
        if let Some(window) = &self.window {
            window.set_visible(false);
        }
        self.bounds.visible = false;
        if let Ok(mut status) = self.status.lock() {
            status.state = "error".into();
            status.message = error;
        }
        self.report();
    }
}

fn native_visibility(hwnd: isize, parent: isize, bounds: Bounds) -> (bool, bool) {
    if hwnd == 0 {
        return (false, false);
    }
    let hwnd = hwnd as windows_sys::Win32::Foundation::HWND;
    // Inspect only our own child window and siblings in the Tauri client area.
    unsafe {
        let visible = IsWindowVisible(hwnd) != 0;
        let frontmost = visible
            && [0.1, 0.5, 0.9].into_iter().all(|x| {
                [0.1, 0.5, 0.9].into_iter().all(|y| {
                    ChildWindowFromPointEx(
                        parent as _,
                        POINT {
                            x: bounds.x + (bounds.width as f64 * x) as i32,
                            y: bounds.y + (bounds.height as f64 * y) as i32,
                        },
                        CWP_SKIPINVISIBLE,
                    ) == hwnd
                })
            });
        (visible, frontmost)
    }
}
impl ApplicationHandler<Message> for Host {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let Some(parent) = NonZeroIsize::new(self.parent) else {
            self.fail("主窗口句柄无效。".into());
            return;
        };
        // Parent is owned by Tauri and remains alive while the host is running.
        let attributes = unsafe {
            Window::default_attributes()
                .with_parent_window(Some(RawWindowHandle::Win32(Win32WindowHandle::new(parent))))
        }
        .with_decorations(false)
        .with_visible(false)
        .with_inner_size(PhysicalSize::new(1, 1))
        .with_title("Itasha GPU viewport");
        let result = event_loop
            .create_window(attributes)
            .map(Arc::new)
            .map_err(|e| e.to_string());
        match result {
            Ok(window) => {
                self.window = Some(window);
                self.report();
            }
            Err(error) => self.fail(error),
        }
    }
    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: Message) {
        match event {
            Message::Gesture { input, reply } => {
                let result = self.gesture_input(*input);
                let _ = reply.send(result);
            }
            Message::Editor {
                epoch,
                revision,
                layers,
            } => {
                self.edit_gesture = None;
                self.gesture_service.invalidate();
                (self.preview_event)(
                    serde_json::json!({"stamp":{"sessionEpoch":self.edit_epoch},"layer":null}),
                );
                self.edit_epoch = epoch;
                self.edit_revision = revision;
                self.edit_layers = layers;
                if !self
                    .edit_layers
                    .iter()
                    .any(|l| Some(l.id.to_string()) == self.selected)
                {
                    self.selected = None;
                }
                if let Some(gpu) = &mut self.gpu {
                    gpu.freeze_control_basis(None);
                    gpu.editor(self.edit_layers.clone(), self.selected.clone());
                }
                self.dirty = true;
                self.report();
            }
            Message::Select { epoch, id } => {
                if epoch == self.edit_epoch
                    && id
                        .as_ref()
                        .is_none_or(|id| self.edit_layers.iter().any(|l| l.id.to_string() == *id))
                {
                    self.selected = id;
                    if let Some(gpu) = &mut self.gpu {
                        gpu.select(self.selected.clone());
                    }
                    self.dirty = true;
                    self.report();
                }
            }
            Message::Probe { x, y, reply } => {
                let value = self
                    .gpu
                    .as_ref()
                    .map(|g| g.probe(x, y))
                    .unwrap_or(serde_json::Value::Null);
                let _ = reply.send(value);
            }
            Message::Pointer(input) => {
                self.pointer_input(input);
            }
            Message::Artwork {
                epoch,
                revision,
                id,
                width,
                height,
                pixels,
                reply,
            } => {
                let valid = self
                    .document_stamp
                    .lock()
                    .is_ok_and(|s| s.as_ref() == Some(&(epoch.clone(), revision)))
                    && epoch == self.edit_epoch
                    && self.edit_layers.iter().any(|l| l.id.to_string() == id);
                if valid && let Some(gpu) = &mut self.gpu {
                    let applied = gpu.artwork_texture(id, width, height, pixels).is_ok();
                    self.dirty = true;
                    let _ = reply.send(applied);
                    self.report();
                } else {
                    let _ = reply.send(false);
                }
            }
            Message::Decals => {
                let updates = self
                    .decals
                    .lock()
                    .map(|mut v| std::mem::take(&mut *v))
                    .unwrap_or_default();
                for (id, update) in updates {
                    let stamp = self
                        .document_stamp
                        .lock()
                        .map(|v| v.clone())
                        .unwrap_or_default();
                    let epoch = self
                        .status
                        .lock()
                        .map(|s| s.session_epoch.clone())
                        .unwrap_or_default();
                    if update.epoch != epoch
                        || update.generation != self.generation
                        || self
                            .last_decal_revision
                            .get(&id)
                            .is_some_and(|v| *v > update.revision)
                        || stamp.as_ref() != Some(&(update.epoch.clone(), update.revision))
                    {
                        let _ = update.reply.send(false);
                        continue;
                    }
                    if let Some(gpu) = &mut self.gpu {
                        if let Err(error) =
                            gpu.update_decal(&id, update.width, update.height, &update.pixels)
                        {
                            let _ = update.reply.send(false);
                            self.fail(error);
                            return;
                        }
                        if update.pixels.chunks_exact(4).any(|p| p[3] != 0) {
                            self.painted_surfaces.insert(id.clone());
                        } else {
                            self.painted_surfaces.remove(&id);
                        }
                        self.last_decal_revision.insert(id, update.revision);
                        if let Ok(mut status) = self.status.lock() {
                            status.decal_updates += 1
                        }
                        self.dirty = true;
                        let _ = update.reply.send(true);
                    } else {
                        let _ = update.reply.send(false);
                    }
                }
                self.report();
            }
            Message::Configure(performance, scene) => {
                self.performance = performance;
                self.scene_settings = scene;
                if let Some(gpu) = &mut self.gpu {
                    gpu.configure(self.performance.clone(), self.scene_settings.clone());
                }
                self.dirty = true;
            }
            Message::Stop => event_loop.exit(),
            Message::Focus => {
                if let Some(gpu) = &mut self.gpu {
                    gpu.focus_selected();
                    self.dirty = true;
                }
            }
            Message::Reset => {
                if let (Some(gpu), Some((min, max))) = (&mut self.gpu, self.reset_bounds) {
                    gpu.camera.reset(min, max);
                    gpu.invalidate_history();
                    self.dirty = true;
                }
            }
            Message::Bounds(bounds) => {
                self.bounds = bounds;
                if let Some(window) = &self.window {
                    window.set_outer_position(PhysicalPosition::new(bounds.x, bounds.y));
                    let _ = window.request_inner_size(PhysicalSize::new(
                        bounds.width.max(1),
                        bounds.height.max(1),
                    ));
                    window.set_visible(bounds.visible && bounds.width > 0 && bounds.height > 0);
                    // A splitter drag must keep receiving pointer events even when
                    // the cursor crosses this native child window, so the window is
                    // made transparent to hit-testing for the duration of the drag.
                    if let Ok(handle) = window.window_handle()
                        && let RawWindowHandle::Win32(handle) = handle.as_raw()
                    {
                        let hwnd = handle.hwnd.get() as _;
                        let style = unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) };
                        let next = if bounds.pass_through {
                            style | WS_EX_TRANSPARENT as isize
                        } else {
                            style & !(WS_EX_TRANSPARENT as isize)
                        };
                        if next != style {
                            unsafe { SetWindowLongPtrW(hwnd, GWL_EXSTYLE, next) };
                        }
                    }
                    if bounds.visible {
                        // WebView2 owns a sibling HWND that can otherwise cover this surface.
                        // Raise only among siblings, without activating the app or making it topmost.
                        if let Ok(handle) = window.window_handle()
                            && let RawWindowHandle::Win32(handle) = handle.as_raw()
                        {
                            let result = unsafe {
                                SetWindowPos(
                                    handle.hwnd.get() as _,
                                    HWND_TOP,
                                    0,
                                    0,
                                    0,
                                    0,
                                    SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE,
                                )
                            };
                            if result == 0 {
                                self.fail(format!(
                                    "无法显示原生 3D 视口：{}",
                                    std::io::Error::last_os_error()
                                ));
                                return;
                            }
                        }
                    }
                }
                if bounds.visible
                    && let Some(gpu) = &mut self.gpu
                {
                    gpu.resize(bounds.width, bounds.height);
                }
                self.button = None;
                self.last_cursor = None;
                self.dirty = true;
                self.report();
            }
            Message::Begin { generation, epoch } => {
                self.gesture_service.invalidate();
                self.edit_gesture = None;
                if let Some(gpu) = &mut self.gpu {
                    gpu.freeze_control_basis(None);
                }
                (self.preview_event)(
                    serde_json::json!({"stamp":{"sessionEpoch":self.edit_epoch},"layer":null}),
                );
                self.last_decal_revision.clear();
                self.painted_surfaces.clear();
                if let Ok(mut cache) = self.surfaces.lock() {
                    *cache = SurfaceCache::default()
                }
                self.generation = generation;
                if let Ok(mut status) = self.status.lock() {
                    status.session_epoch = epoch.clone();
                    status.generation = generation;
                    status.state = "loading".into();
                    status.stats = None;
                    status.surfaces.clear();
                    status.frames = 0;
                }
                if self.gpu.is_none()
                    && let Some(window) = &self.window
                {
                    match itasha_renderer::pollster::block_on(Gpu::new(window.clone())) {
                        Ok(mut gpu) => {
                            gpu.configure(self.performance.clone(), self.scene_settings.clone());
                            gpu.resize(self.bounds.width, self.bounds.height);
                            self.gpu = Some(gpu);
                        }
                        Err(error) => {
                            self.fail(error);
                            return;
                        }
                    }
                }
                if let Some(gpu) = &mut self.gpu {
                    gpu.clear();
                }
                self.reset_bounds = None;
                if let Ok(mut status) = self.status.lock() {
                    status.state = "loading".into();
                    status.message = "正在读取 GLB 与 PBR 材质…".into();
                    status.session_epoch = epoch;
                    status.stats = None;
                    status.frames = 0;
                }
                self.dirty = true;
                self.report();
            }
            Message::Loaded {
                generation,
                scene,
                started,
            } => {
                if generation != self.generation {
                    return;
                }
                match *scene {
                    Ok(scene) => {
                        if let (Ok(mut cache), Ok(mut status)) =
                            (self.surfaces.lock(), self.status.lock())
                        {
                            cache.epoch = status.session_epoch.clone();
                            cache.generation = generation;
                            cache.fingerprint = scene.stats.surface_fingerprint.clone();
                            cache.topology =
                                Some(Arc::new(itasha_surface_map::topology::Topology::new(
                                    scene
                                        .surfaces
                                        .iter()
                                        .flat_map(|s| s.editable_triangles.clone())
                                        .collect(),
                                    &scene.paint_connections,
                                )));
                            status.surfaces =
                                scene.surfaces.iter().map(|s| s.info.clone()).collect();
                            cache.masks = scene
                                .surfaces
                                .iter()
                                .map(|s| (s.info.id.clone(), s.mask.clone()))
                                .collect();
                        }
                        if let Some(gpu) = &mut self.gpu {
                            self.reset_bounds = Some((scene.min, scene.max));
                            let (stats, upload) = gpu.upload(scene);
                            gpu.control_scale = self
                                .window
                                .as_ref()
                                .map(|w| w.scale_factor() as f32)
                                .unwrap_or(1.);
                            gpu.editor(self.edit_layers.clone(), self.selected.clone());
                            if let Ok(mut status) = self.status.lock() {
                                status.state = "ready".into();
                                status.message = "静态 PBR 预览 · 原生 Rust/wgpu".into();
                                status.stats = Some(stats);
                                status.upload_cpu_ms = upload;
                                status.ready_ms = started.elapsed().as_secs_f64() * 1000.;
                            }
                            self.dirty = true;
                            self.report();
                        } else {
                            self.fail("GPU 设备尚未就绪，请重新载入。".into());
                        }
                    }
                    Err(error) => self.fail(error),
                }
            }
        }
    }
    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::DroppedFile(path) => {
                let mut point = POINT { x: 0, y: 0 };
                let hwnd = self.status.lock().ok().map(|s| s.native_hwnd).unwrap_or(0);
                if hwnd != 0
                    && unsafe { GetCursorPos(&mut point) } != 0
                    && unsafe { ScreenToClient(hwnd as _, &mut point) } != 0
                {
                    let probe = self
                        .gpu
                        .as_ref()
                        .map(|g| g.probe(point.x as f32, point.y as f32))
                        .unwrap_or(serde_json::Value::Null);
                    let drop = if probe.is_null() {
                        serde_json::Value::Null
                    } else {
                        serde_json::json!({"surfaceId":probe["surfaceId"],"xMm":probe["localPoint"][0],"yMm":probe["localPoint"][1]})
                    };
                    (self.file_drop)(
                        serde_json::json!({"sessionEpoch":self.edit_epoch,"paths":[path],"point":drop}),
                    );
                }
            }
            WindowEvent::Destroyed => event_loop.exit(),
            WindowEvent::Occluded(true) => {
                self.button = None;
                self.last_cursor = None;
            }
            WindowEvent::Resized(size) => {
                if self.bounds.visible
                    && let Some(gpu) = &mut self.gpu
                {
                    gpu.resize(size.width, size.height);
                }
                self.dirty = true;
            }
            WindowEvent::CursorMoved { position, .. } => {
                if self.pointer_input(PointerInput {
                    kind: "move".into(),
                    x: position.x as f32,
                    y: position.y as f32,
                    shift: self.modifiers.shift_key(),
                }) {
                    self.last_cursor = Some((position.x, position.y));
                    return;
                }
                if let (Some((x, y)), Some(button), Some(gpu)) =
                    (self.last_cursor, self.button, &mut self.gpu)
                {
                    let scale = self.window.as_ref().map(|w| w.scale_factor()).unwrap_or(1.) as f32;
                    let dx = (position.x - x) as f32 / scale;
                    let dy = (position.y - y) as f32 / scale;
                    if button == MouseButton::Left {
                        gpu.camera.orbit(dx, dy);
                    } else {
                        gpu.camera.pan(dx, dy);
                    }
                    self.dirty = true;
                }
                self.last_cursor = Some((position.x, position.y));
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if button == MouseButton::Left
                    && !self.modifiers.alt_key()
                    && let Some((x, y)) = self.last_cursor
                {
                    let handled = self.pointer_input(PointerInput {
                        kind: if state == ElementState::Pressed {
                            "down"
                        } else {
                            "up"
                        }
                        .into(),
                        x: x as f32,
                        y: y as f32,
                        shift: self.modifiers.shift_key(),
                    });
                    if handled {
                        self.button = None;
                        return;
                    }
                }
                if state == ElementState::Pressed {
                    if button == MouseButton::Left
                        && self.last_click.elapsed() < Duration::from_millis(300)
                        && let (Some(gpu), Some((min, max))) = (&mut self.gpu, self.reset_bounds)
                    {
                        gpu.camera.reset(min, max);
                        gpu.invalidate_history();
                        self.dirty = true;
                    }
                    self.last_click = Instant::now();
                    self.button = Some(button);
                } else {
                    self.button = None;
                }
            }
            WindowEvent::Focused(false) => {
                self.pointer_input(PointerInput {
                    kind: "cancel".into(),
                    x: 0.,
                    y: 0.,
                    shift: false,
                });
                self.button = None;
                self.last_cursor = None;
                self.modifiers = ModifiersState::empty();
            }
            WindowEvent::KeyboardInput { event, .. }
                if event.state == ElementState::Pressed
                    && event.logical_key == Key::Named(winit::keyboard::NamedKey::Escape) =>
            {
                self.pointer_input(PointerInput {
                    kind: "cancel".into(),
                    x: 0.,
                    y: 0.,
                    shift: false,
                });
            }
            WindowEvent::ModifiersChanged(modifiers) => self.modifiers = modifiers.state(),
            WindowEvent::KeyboardInput { event, .. }
                if event.state == ElementState::Pressed
                    && !event.repeat
                    && self.modifiers.control_key()
                    && !self.modifiers.alt_key()
                    && !self.modifiers.super_key() =>
            {
                if let Key::Character(character) = event.logical_key {
                    let key = if character == "," {
                        "Comma".into()
                    } else {
                        character.to_uppercase()
                    };
                    if key == "Comma" || (key.len() == 1 && key.as_bytes()[0].is_ascii_uppercase())
                    {
                        (self.shortcut)(format!(
                            "Ctrl+{}{key}",
                            if self.modifiers.shift_key() {
                                "Shift+"
                            } else {
                                ""
                            }
                        ));
                    }
                }
            }
            WindowEvent::CursorLeft { .. } => {
                self.pointer_input(PointerInput {
                    kind: "cancel".into(),
                    x: 0.,
                    y: 0.,
                    shift: false,
                });
                self.last_cursor = None;
                self.button = None;
            }
            WindowEvent::MouseWheel { delta, .. } => {
                if let Some(gpu) = &mut self.gpu {
                    gpu.camera.zoom(match delta {
                        MouseScrollDelta::LineDelta(_, y) => y * 100.,
                        MouseScrollDelta::PixelDelta(p) => p.y as f32,
                    });
                    self.dirty = true;
                }
            }
            WindowEvent::RedrawRequested if self.bounds.visible => {
                let frame_started = Instant::now();
                if let Some(gpu) = &mut self.gpu
                    && let Err(error) = gpu.render(self.bounds.light)
                {
                    self.fail(error);
                    return;
                }
                self.dirty = self.gpu.as_ref().is_some_and(|gpu| gpu.temporal_pending());
                self.last_frame = frame_started;
                let first_frame = self.status.lock().map(|s| s.frames == 0).unwrap_or(false);
                let mut settings_changed = false;
                if let (Some(gpu), Ok(mut status)) = (&self.gpu, self.status.lock()) {
                    settings_changed = status.antialiasing != self.performance.antialiasing;
                    status.frames = gpu.frames;
                    (status.render_width, status.render_height) = gpu.render_size();
                    status.target_fps = self.performance.target_fps;
                    status.antialiasing = self.performance.antialiasing.clone();
                    status.temporal_samples = gpu.temporal_samples();
                    status.temporal_pending = gpu.temporal_pending();
                    status.render_scale_limited = gpu.render_scale_limited();
                    status.scene_time_minutes = self.scene_settings.time_minutes;
                    status.skybox = self.scene_settings.skybox;
                    status.camera_eye = gpu
                        .camera
                        .eye(self.bounds.width as f32 / self.bounds.height.max(1) as f32)
                        .to_array();
                    status.camera_distance = gpu.camera.distance;
                    status.camera_radius = gpu.camera.radius;
                    status.last_submit_cpu_ms = gpu.last_submit_ms;
                    status.control_points = gpu.control_points();
                    status.painted_surfaces = gpu.painted_parts();
                    status.selected_layer = gpu.selected_id();
                }
                let temporal_finished = self
                    .gpu
                    .as_ref()
                    .is_some_and(|gpu| gpu.temporal_samples() > 0 && !gpu.temporal_pending());
                if first_frame
                    || settings_changed
                    || temporal_finished
                    || self.last_publish.elapsed() > Duration::from_millis(500)
                {
                    self.report();
                }
            }
            _ => {}
        }
    }
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.dirty && self.bounds.visible {
            let next = self.last_frame
                + Duration::from_secs_f64(1. / f64::from(self.performance.target_fps));
            if Instant::now() >= next {
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            } else {
                event_loop.set_control_flow(ControlFlow::WaitUntil(next));
                return;
            }
        }
        event_loop.set_control_flow(ControlFlow::Wait);
    }
}
