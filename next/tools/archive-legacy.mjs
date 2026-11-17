import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const git = (...args) => execFileSync('git', ['-C', root, ...args], { encoding: 'utf8', windowsHide: true });
const hash = bytes => crypto.createHash('sha256').update(bytes).digest('hex');
const destinationArg = process.argv.indexOf('--destination');
const stamp = new Date().toISOString().replace(/[:.]/g, '-');
const destination = destinationArg >= 0
  ? path.resolve(process.argv[destinationArg + 1])
  : path.resolve(root, '..', '_归档', `legacy-electron-0.2-${stamp}`);
const relativeDestination = path.relative(root, destination);
if (!relativeDestination || (!relativeDestination.startsWith('..' + path.sep) && relativeDestination !== '..' && !path.isAbsolute(relativeDestination))) throw Error('Archive must be outside the active repository');
if (fs.existsSync(destination)) throw Error('Archive destination already exists; refusing overwrite');
const files = new Set(git('ls-files', '-z').split('\0').filter(Boolean));
for (const file of git('ls-files', '--others', '--exclude-standard', '-z', '--', 'docs').split('\0').filter(Boolean)) files.add(file);
const selected = [...files].filter(f => !f.startsWith('next/') && !f.startsWith('archives/')).sort();
fs.mkdirSync(path.dirname(destination), { recursive: true });
fs.mkdirSync(destination);
fs.mkdirSync(path.join(destination, 'source'));
const entries = [];
for (const name of selected) {
  const source = path.join(root, name);
  if (!fs.existsSync(source) || !fs.lstatSync(source).isFile()) throw Error(`Missing/nonregular source: ${name}`);
  const bytes = fs.readFileSync(source);
  const target = path.join(destination, 'source', name);
  fs.mkdirSync(path.dirname(target), { recursive: true });
  fs.writeFileSync(target, bytes, { flag: 'wx' });
  if (hash(fs.readFileSync(target)) !== hash(bytes)) throw Error(`Copy verification failed: ${name}`);
  entries.push({ path: name, bytes: bytes.length, sha256: hash(bytes) });
}
const bundle = path.join(destination, 'history.bundle');
git('bundle', 'create', bundle, '--all');
const bundleVerification = git('bundle', 'verify', bundle);
const manifest = {
  schemaVersion: 1, createdAt: new Date().toISOString(), sourceRoot: root,
  sourceHead: git('rev-parse', 'HEAD').trim(), branch: git('branch', '--show-current').trim(),
  worktreeStatus: git('status', '--short', '--untracked-files=normal'),
  coverage: 'Git tracked worktree bytes plus untracked docs; full referenced Git history',
  excluded: ['next/', 'user projects and exports', 'browser profiles and secrets', 'ignored vehicle archives', 'downloaded runtimes and build outputs'],
  files: entries, bundle: { path: 'history.bundle', bytes: fs.statSync(bundle).size, sha256: hash(fs.readFileSync(bundle)) },
  bundleVerification: bundleVerification.trim(),
};
fs.writeFileSync(path.join(destination, 'manifest.json'), JSON.stringify(manifest, null, 2) + '\n', { flag: 'wx' });
fs.writeFileSync(path.join(destination, 'RESTORE.md'), [
  '# Legacy 0.2 archive', '',
  'source/ contains the captured worktree and design documents; manifest.json contains SHA-256 hashes.',
  'history.bundle contains Git history. Restore into a NEW directory:', '',
  '    git clone history.bundle restored-repository', '',
  'Overlay source/ onto the new clone to restore captured worktree edits and untracked docs.',
  'Do not overlay onto an active checkout containing user edits.',
  'Ignored offline models, user projects, exports, browser profiles, credentials, and binaries were deliberately not copied.',
  'They remain at the original location. Use the legacy local-assets tool to restore models from that location.',
  'Read source/docs/开发与构建.md to rebuild the legacy application.', '',
].join('\n'), { flag: 'wx' });
console.log(JSON.stringify({ destination, files: entries.length, sourceBytes: entries.reduce((n, e) => n + e.bytes, 0), head: manifest.sourceHead, bundleVerified: true }, null, 2));
