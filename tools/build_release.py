"""Compatibility entry point for the Rust/Electron portable packager."""
import runpy
from pathlib import Path

if __name__ == "__main__":
    runpy.run_path(str(Path(__file__).with_name("package-desktop.py")), run_name="__main__")
