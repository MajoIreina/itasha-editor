use anyhow::{Context, Result, bail};
use itasha_backend::{AppState, http};
use std::{path::PathBuf, sync::Arc};

fn main() -> Result<()> {
    let mut root = std::env::current_dir()?;
    let mut data = None;
    let mut port: u16 = 0;
    let mut gpu = true;
    let mut parent = None;
    let mut token = std::env::var("ITASHA_SESSION_TOKEN").unwrap_or_default();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--root" => root = PathBuf::from(args.next().context("--root requires a path")?),
            "--data-dir" => {
                data = Some(PathBuf::from(
                    args.next().context("--data-dir requires a path")?,
                ))
            }
            "--port" => port = args.next().context("--port requires a number")?.parse()?,
            "--token" => token = args.next().context("--token requires a value")?,
            "--gpu-disabled" => gpu = false,
            "--parent-pid" => {
                parent = Some(
                    args.next()
                        .context("--parent-pid requires a PID")?
                        .parse::<u32>()?,
                )
            }
            "--version" => {
                println!("itasha-backend {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            _ => bail!("Unknown argument: {arg}"),
        }
    }
    let root = root
        .canonicalize()
        .context("Application root does not exist")?;
    if let Some(parent) = parent {
        itasha_backend::process::watch_parent(parent)?;
    }
    let data = data.unwrap_or_else(|| root.clone());
    std::fs::create_dir_all(&data)?;
    let state = AppState::new(root, data.canonicalize()?, token, gpu)?;
    let server =
        Arc::new(tiny_http::Server::http(("127.0.0.1", port)).map_err(|e| anyhow::anyhow!("{e}"))?);
    let address = server
        .server_addr()
        .to_ip()
        .context("Missing TCP address")?;
    println!(
        "ITASHA_READY {}",
        serde_json::json!({"port":address.port(),"pid":std::process::id(),"backend":"rust","version":env!("CARGO_PKG_VERSION")})
    );
    // Bound HTTP concurrency; image concurrency has its own configurable gate.
    let workers = std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(4)
        .clamp(4, 16);
    std::thread::scope(|scope| {
        for _ in 0..workers {
            let server = server.clone();
            let state = state.clone();
            scope.spawn(move || {
                for request in server.incoming_requests() {
                    http::handle(request, &state);
                }
            });
        }
    });
    Ok(())
}
