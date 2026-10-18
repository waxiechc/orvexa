// Cross-platform counterpart of collect-licenses.ps1. Build-time only.
import { spawnSync } from 'node:child_process';
import { readFileSync, readdirSync, existsSync, mkdirSync, copyFileSync, writeFileSync } from 'node:fs';
import { resolve, dirname, join } from 'node:path';
import { createHash } from 'node:crypto';
const output = resolve(process.argv[2] || 'release/third-party-licenses');
const targets = process.argv.slice(3);
if (!targets.length) throw new Error('Specify at least one Cargo target');
function cargo(args) { const r = spawnSync('cargo', args, { encoding: 'utf8', maxBuffer: 32 * 1024 * 1024 }); if (r.status !== 0) throw new Error(r.stderr || 'Cargo metadata failed'); return r.stdout; }
const metadata = JSON.parse(cargo(['metadata', '--locked', '--format-version', '1']));
const selected = new Set(cargo(['tree', '--locked', '--workspace', ...targets.flatMap(t => ['--target', t]), '--edges', 'normal,build,dev', '--prefix', 'none', '--format', '{p}']).split('\n').map(l => l.match(/^(\S+) v(\S+)/)).filter(Boolean).map(m => `${m[1]}@${m[2]}`));
const index = [];
mkdirSync(output, { recursive: true });
function copy(kind, name, version, folder, license, extra) {
  let files = readdirSync(folder, { withFileTypes: true }).filter(f => f.isFile() && /^(licen[cs]e|copying|notice|copyright|authors)([._-].*)?$/i.test(f.name)).map(f => join(folder, f.name));
  if (extra && existsSync(extra)) files.push(extra);
  if (!files.length && kind === 'cargo') {
    const pinned = resolve('docs/third-party-notices', `${name}-${version}`);
    if (existsSync(join(pinned, 'source.json'))) {
      const source = JSON.parse(readFileSync(join(pinned, 'source.json')));
      if (source.package !== name || source.version !== version || source.declared_license !== license) throw new Error('Pinned notice metadata mismatch');
      for (const entry of source.sources) if (createHash('sha256').update(readFileSync(join(pinned, entry.file))).digest('hex') !== entry.sha256) throw new Error('Pinned notice hash mismatch');
      files = readdirSync(pinned).map(f => join(pinned, f));
    }
  }
  if (!files.length) throw new Error(`Missing upstream license: ${kind} ${name} ${version}`);
  const destination = join(output, `${kind}-${name}-${version}`.replace(/[^a-z0-9._-]/gi, '_'));
  mkdirSync(destination, { recursive: true });
  for (const file of new Set(files)) copyFileSync(file, join(destination, file.split(/[\\/]/).at(-1)));
  index.push({ package: name, version, ecosystem: kind, declared_license: license, notices: [...new Set(files.map(f => f.split(/[\\/]/).at(-1)))] });
}
for (const p of metadata.packages) if (p.source && selected.has(`${p.name}@${p.version}`)) copy('cargo', p.name, p.version, dirname(p.manifest_path), p.license, p.license_file);
const lock = JSON.parse(readFileSync('web/package-lock.json'));
for (const key of Object.keys(lock.packages)) {
  if (!key.startsWith('node_modules/')) continue;
  const folder = resolve('web', key); if (!existsSync(join(folder, 'package.json'))) continue;
  const p = JSON.parse(readFileSync(join(folder, 'package.json')));
  let extra;
  if (p.name.startsWith('@rolldown/binding-')) {
    const parent = JSON.parse(readFileSync('web/node_modules/rolldown/package.json'));
    if (parent.version !== p.version) throw new Error('Rolldown license version mismatch');
    extra = resolve('web/node_modules/rolldown/LICENSE');
  }
  copy('npm', p.name, p.version, folder, p.license || '', extra);
}
writeFileSync(join(output, 'index.json'), JSON.stringify(index, null, 2));
const combined = ['ORVEXA: лицензии исходного ПО / open-source notices', readFileSync('LICENSE', 'utf8')];
for (const dir of readdirSync(output, { withFileTypes: true }).filter(d => d.isDirectory()).sort((a, b) => a.name.localeCompare(b.name)))
  for (const file of readdirSync(join(output, dir.name)).sort()) combined.push(`\n--- ${dir.name}/${file} ---\n`, readFileSync(join(output, dir.name, file), 'utf8'));
writeFileSync('web/public/open-source-notices.txt', combined.join('\n'));
console.log(`Collected notices: ${index.length} packages`);
