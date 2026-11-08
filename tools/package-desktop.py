"""Portable Rust/Electron package using an explicit runtime allowlist.

Outputs stay inside this workspace; the old WebView2 installer is never used.
"""
import argparse
import hashlib
import json
from pathlib import Path
import zipfile

ROOT = Path(__file__).resolve().parents[1]


def runtime_files():
    files = set()

    def add(relative, required=True):
        p = ROOT / relative
        if not p.is_file():
            if required:
                raise RuntimeError(f"缺少运行文件，请先构建：{p}")
            return
        files.add(p)

    def tree(relative):
        base = ROOT / relative
        if not base.is_dir():
            raise RuntimeError(f"缺少资源目录：{base}")
        files.update(p for p in base.rglob('*') if p.is_file())

    for relative in ('app', 'config', 'docs', 'resources/data', 'resources/ui', 'vehicles/data', 'desktop/electron'):
        tree(relative)
    for relative in ('resources/manifest.json', 'resources/extras.json', 'resources/custom.js', 'resources/custom.css',
                     'desktop/build-manifest.json', 'desktop/native/itasha-backend.exe', '痛车编辑器.exe',
                     'SU7涂装工作室.exe', '启动工作室.cmd', '关闭后台服务.cmd', '关闭后台服务.ps1',
                     '使用说明.md', 'NOTICE.txt', 'README.md', 'projects/示例-蓝曜流线.su7design'):
        add(relative)
    registry = json.loads((ROOT / 'config/vehicles.json').read_text(encoding='utf-8'))
    for vehicle in registry['vehicles']:
        add(vehicle['manifest'])
        add(vehicle['adapter'])
        if vehicle.get('extras'):
            add(vehicle['extras'])
        for name in ('adapter.json', 'acceptance.json', 'source.json'):
            add(f"vehicles/{vehicle['id']}/{name}", required=False)
    native = ROOT / 'desktop/native/imagemagick'
    add('desktop/native/imagemagick/magick.exe')
    for p in native.iterdir():
        if p.is_file() and p.suffix.lower() in ('.xml', '.icc', '.txt'):
            files.add(p)
    banned = {'node_modules', 'target', '__pycache__', 'app-data-electron', 'webview2-data', 'webview2-test-data', 'exports', 'logs'}
    for p in files:
        relative = p.relative_to(ROOT)
        if banned.intersection(relative.parts) or p.suffix.lower() in ('.py', '.pyc', '.pdb'):
            raise RuntimeError(f"发布清单意外包含开发文件：{relative}")
    return sorted(files)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--check', action='store_true', help='validate allowlist without packaging')
    args = parser.parse_args()
    files = runtime_files()
    result = {'version': '0.2.0', 'runtime': 'Rust + Electron', 'files': len(files),
              'sourceBytes': sum(p.stat().st_size for p in files), 'allowlist': 'passed'}
    if not args.check:
        output = ROOT / 'development/dist/痛车编辑器-0.2.0-便携版.zip'
        output.parent.mkdir(parents=True, exist_ok=True)
        pending = output.with_suffix('.zip.pending')
        with zipfile.ZipFile(pending, 'w', zipfile.ZIP_DEFLATED, compresslevel=1) as archive:
            for p in files:
                archive.write(p, str(Path('痛车编辑器') / p.relative_to(ROOT)))
            archive.writestr('痛车编辑器/exports/', '')
        with zipfile.ZipFile(pending) as archive:
            bad = archive.testzip()
            if bad:
                raise RuntimeError(f'ZIP 校验失败：{bad}')
        pending.replace(output)
        with output.open('rb') as source:
            sha256 = hashlib.file_digest(source, 'sha256').hexdigest()
        output.with_suffix('.zip.sha256').write_text(f'{sha256}  {output.name}\n', encoding='utf-8')
        result.update(file=str(output), bytes=output.stat().st_size, sha256=sha256, integrity='passed')
    report = ROOT / 'tools/acceptance/packaging/verification.json'
    report.parent.mkdir(parents=True, exist_ok=True)
    report.write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding='utf-8')
    print(json.dumps(result, ensure_ascii=False, indent=2))


if __name__ == '__main__':
    main()
