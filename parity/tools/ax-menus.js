// Dump the full menu tree of a running app via System Events accessibility.
// Usage: osascript -l JavaScript menus.js <ProcessName>
function run(argv) {
  const app = argv[0] || 'AdobeReader';
  const se = Application('System Events');
  const proc = se.processes[app];
  const mb = proc.menuBars[0];
  const out = [];
  const bars = mb.menuBarItems;
  for (let i = 0; i < bars.length; i++) {
    const bi = bars[i];
    let name;
    try { name = bi.name(); } catch (e) { name = '<?>'; }
    if (name === 'Apple') continue;
    out.push({ depth: 0, kind: 'menubar', index: i, title: name });
    try { walk(bi.menus[0], 1, out); } catch (e) { out.push({ depth: 1, kind: 'error', title: String(e) }); }
  }
  return JSON.stringify(out, null, 1);
}

function walk(menu, depth, out) {
  const items = menu.menuItems;
  const n = items.length;
  for (let j = 0; j < n; j++) {
    const it = items[j];
    const rec = { depth: depth, index: j };
    try { rec.title = it.name(); } catch (e) { rec.title = null; }
    if (rec.title === null || rec.title === '') { rec.kind = 'separator'; out.push(rec); continue; }
    rec.kind = 'item';
    try { rec.enabled = it.enabled(); } catch (e) { }
    try { rec.mark = it.attributes['AXMenuItemMarkChar'].value(); } catch (e) { }
    try { rec.cmdchar = it.attributes['AXMenuItemCmdChar'].value(); } catch (e) { }
    try { rec.cmdmod = it.attributes['AXMenuItemCmdModifiers'].value(); } catch (e) { }
    let sub = null;
    try { sub = it.menus.length ? it.menus[0] : null; } catch (e) { }
    if (sub) { rec.submenu = true; out.push(rec); walk(sub, depth + 1, out); }
    else out.push(rec);
  }
}
