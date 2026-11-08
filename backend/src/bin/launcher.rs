#![windows_subsystem = "windows"]
use std::{env, process::Command};

fn main() {
    let Ok(exe) = env::current_exe() else { return };
    let Some(root) = exe.parent() else { return };
    let electron = root.join("desktop/electron/痛车编辑器.exe");
    let result = Command::new(electron)
        .current_dir(root)
        .args(env::args_os().skip(1))
        .spawn();
    if let Err(error) = result {
        let message = format!(
            "无法启动痛车编辑器：{error}\n请运行 tools/build-desktop.cjs 重新构建桌面程序。"
        );
        let _ = std::fs::write(root.join("启动错误.txt"), &message);
        #[cfg(windows)]
        {
            let _ = Command::new("notepad.exe")
                .arg(root.join("启动错误.txt"))
                .spawn();
        }
    }
}
