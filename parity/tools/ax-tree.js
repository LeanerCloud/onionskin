// Dump the accessibility tree of a running app's windows and sheets.
//
// Usage: osascript -l JavaScript ax-tree.js <ProcessName> [maxDepth]
//
// Emits JSON lines of {depth, role, subrole, title, value, desc, help, enabled,
// focused, selected, pos, size}. Geometry is in points, screen coordinates; only
// relative differences between nodes in one dump are meaningful, per the
// relative-measurement rule in docs/plans/parity-goal.md section 4.3.
function run(argv) {
  const app = argv[0] || 'AdobeReader';
  const maxDepth = argv[1] ? parseInt(argv[1], 10) : 12;
  const se = Application('System Events');
  const proc = se.processes[app];
  const out = [];
  const roots = proc.windows;
  for (let i = 0; i < roots.length; i++) {
    walk(roots[i], 0, maxDepth, out, 'window[' + i + ']');
  }
  return JSON.stringify(out, null, 1);
}

function attr(el, name) {
  try { return el.attributes[name].value(); } catch (e) { return undefined; }
}

function walk(el, depth, maxDepth, out, path) {
  if (depth > maxDepth) return;
  const rec = { depth: depth, path: path };
  rec.role = attr(el, 'AXRole');
  const sub = attr(el, 'AXSubrole');
  if (sub) rec.subrole = sub;
  const t = attr(el, 'AXTitle');
  if (t) rec.title = t;
  const d = attr(el, 'AXDescription');
  if (d) rec.desc = d;
  const h = attr(el, 'AXHelp');
  if (h) rec.help = h;
  const v = attr(el, 'AXValue');
  if (v !== undefined && v !== null && typeof v !== 'object') rec.value = String(v).slice(0, 200);
  const en = attr(el, 'AXEnabled');
  if (en === false) rec.enabled = false;
  if (attr(el, 'AXFocused') === true) rec.focused = true;
  const sel = attr(el, 'AXSelected');
  if (sel === true) rec.selected = true;
  const p = attr(el, 'AXPosition');
  const s = attr(el, 'AXSize');
  if (p) rec.pos = [p.x, p.y];
  if (s) rec.size = [s.width, s.height];
  const ident = attr(el, 'AXIdentifier');
  if (ident) rec.id = ident;
  out.push(rec);

  let kids = [];
  try { kids = el.uiElements; } catch (e) { return; }
  let n = 0;
  try { n = kids.length; } catch (e) { return; }
  for (let i = 0; i < n; i++) {
    walk(kids[i], depth + 1, maxDepth, out, path + '/' + i);
  }
}
