const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const { spawn } = require('node:child_process');
const readline = require('node:readline');

class BackendProcess {
  constructor(config) { this.config = config; this.child = null; this.stopping = false; this.token = crypto.randomBytes(32).toString('hex'); }
  async start(onExit) {
    const { root, data, port, gpuAtStartup } = this.config;
    fs.mkdirSync(path.join(data, 'logs'), { recursive: true });
    const log = fs.createWriteStream(path.join(data, 'logs', 'backend.log'), { flags: 'a' });
    const executable = path.join(root, 'desktop', 'native', 'itasha-backend.exe');
    if (!fs.existsSync(executable)) throw new Error(`缺少 Rust 后端：${executable}`);
    const args = ['--root', root, '--data-dir', data, '--port', String(port), '--parent-pid', String(process.pid)];
    if (!gpuAtStartup) args.push('--gpu-disabled');
    const child = this.child = spawn(executable, args, { cwd: root, windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'], env: { ...process.env, ITASHA_SESSION_TOKEN: this.token } });
    child.stderr.pipe(log, { end: false });
    child.once('exit', (code, signal) => { log.end(); if (!this.stopping) onExit?.(code, signal); });
    return new Promise((resolve, reject) => {
      const timeout = setTimeout(() => { this.stop(); reject(new Error('Rust 后端启动超时，请查看 logs/backend.log')); }, 30000);
      const lines = readline.createInterface({ input: child.stdout });
      let settled = false;
      const fail = error => { if (!settled) { settled = true; clearTimeout(timeout); reject(error); } };
      child.once('error', fail);
      child.once('exit', code => fail(new Error(`Rust 后端启动失败（${code}），请查看 logs/backend.log`)));
      lines.on('line', line => {
        if (!line.startsWith('ITASHA_READY ')) { log.write(line + '\n'); return; }
        try {
          const ready = JSON.parse(line.slice('ITASHA_READY '.length));
          if (ready.backend !== 'rust' || !Number.isInteger(ready.port) || ready.port < 1 || ready.port > 65535) throw new Error('后端启动响应无效');
          settled = true; clearTimeout(timeout); this.origin = `http://127.0.0.1:${ready.port}`;
          this.pid = ready.pid;
          fetch(this.origin + '/__studio_health').then(async response => {
            const health = await response.json();
            if (!response.ok || health.backend !== 'rust') throw new Error('Rust 后端健康检查未通过');
            resolve({ ...ready, origin: this.origin, token: this.token });
          }).catch(reject);
        } catch (error) { fail(error); }
      });
    });
  }
  async stop() {
    this.stopping = true;
    const child = this.child;
    if (!child || child.exitCode !== null || child.signalCode !== null) return;
    await new Promise(resolve => { const timer = setTimeout(resolve, 3000); child.once('exit', () => { clearTimeout(timer); resolve(); }); child.kill(); });
    this.child = null;
  }
}
module.exports = { BackendProcess };
