'use strict';

const TERMINAL_DEMO_MODE = true;

class TerminalSession {
  constructor(id, label, backend) {
    this.id = id;
    this.label = label;
    this.backend = backend;
    this.term = null;
    this._dataHandlers = [];
    this._disposed = false;
  }

  attach(el) {
    if (typeof Terminal === 'undefined') {
      el.innerHTML = '<div class="term-error">xterm.js failed to load from CDN.</div>';
      return;
    }
    this.term = new Terminal({
      cursorBlink: true,
      scrollback: 10000,
      fontFamily: 'ui-monospace, "Cascadia Code", Menlo, Consolas, monospace',
      fontSize: 13,
      theme: {
        background: '#0d1117',
        foreground: '#e6edf3',
        cursor: '#79c0ff',
        selectionBackground: 'rgba(31, 111, 235, 0.35)',
      },
    });
    this.term.open(el);

    this.backend.onData((data) => {
      if (!this._disposed) this.term.write(data);
    });

    this.term.onData((data) => {
      this.backend.write(data);
      for (const h of this._dataHandlers) h(data);
    });

    this._fit();
    new ResizeObserver(() => this._fit()).observe(el);
    this.term.focus();
  }

  onData(cb) { this._dataHandlers.push(cb); }

  write(data) {
    if (this.term && !this._disposed) this.term.write(data);
  }

  resize(cols, rows) {
    if (this.term) this.term.resize(cols, rows);
    this.backend.resize(cols, rows);
  }

  close() {
    this._disposed = true;
    try { this.backend.close(); } catch {  }
    if (this.term) this.term.dispose();
  }

  _fit() {
    if (!this.term) return;

    const el = this.term.element;
    if (!el) return;
    const dims = this.term.dimensions;
    if (!dims || !dims.actualCellWidth) return;
    const rect = el.getBoundingClientRect();
    const cols = Math.max(20, Math.floor(rect.width / dims.actualCellWidth));
    const rows = Math.max(5, Math.floor(rect.height / dims.actualCellHeight));
    this.resize(cols, rows);
  }
}

class FakeShell {
  constructor(session) {
    this.session = session;
    this._buf = '';
    this._cbs = [];
    this._dead = false;
  }
  onData(cb) { this._cbs.push(cb); }
  write(data) {
    if (this._dead) return;
    for (const ch of data) this._handleChar(ch);
  }
  resize() {  }
  close() { this._dead = true; }

  _emit(s) { for (const cb of this._cbs) cb(s); }

  _handleChar(ch) {
    if (ch === '\r') {
      this._emit('\r\n');
      this._runCommand(this._buf);
      this._buf = '';
      if (!this._dead) this._emit('$ ');
    } else if (ch === '\u007f') {
      if (this._buf.length > 0) {
        this._buf = this._buf.slice(0, -1);
        this._emit('\b \b');
      }
    } else if (ch >= ' ' || ch === '\t') {
      this._buf += ch;
      this._emit(ch);
    }

  }

  _runCommand(line) {
    const [cmd, ...args] = line.trim().split(/\s+/).filter(Boolean);
    if (!cmd) return;
    if (cmd === 'exit') {
      this._emit('logout\r\n');
      this._dead = true;
      return;
    }
    if (cmd === 'clear') {
      this.session.term?.clear();
      return;
    }
    if (cmd === 'help') {
      this._emit('demo shell commands: help, clear, echo <text>, exit\r\n');
      return;
    }
    if (cmd === 'echo') {
      this._emit(args.join(' ') + '\r\n');
      return;
    }

    this._emit(`$ ${line}\r\n(executed in demo mode - wire portable-pty for a real shell)\r\n`);
  }

  greet() {
    this._emit('Zen terminal (demo shell - no real backend yet)\r\n');
    this._emit('Type `help` for demo commands. The agent cannot type here.\r\n$ ');
  }
}

function createBackend(session) {
  if (TERMINAL_DEMO_MODE) return new FakeShell(session);

  throw new Error('portable-pty backend not wired yet');
}

class TerminalManager {
  constructor(tabsEl, panesEl) {
    this.tabsEl = tabsEl;
    this.panesEl = panesEl;
    this.sessions = new Map();
    this.activeId = null;
    this._nextId = 1;
  }

  newTab() {
    const id = 'term-' + this._nextId++;
    const label = 'Terminal ' + (this._nextId - 1);
    const session = new TerminalSession(id, label, null);
    session.backend = createBackend(session);
    this.sessions.set(id, session);

    const tab = document.createElement('button');
    tab.className = 'term-tab';
    tab.dataset.tabId = id;
    tab.innerHTML = `<span>${label}</span><span class="term-tab-close" title="Close">×</span>`;
    tab.querySelector('.term-tab-close').addEventListener('click', (e) => {
      e.stopPropagation();
      this.closeTab(id);
    });
    tab.addEventListener('click', () => this.activate(id));
    this.tabsEl.appendChild(tab);

    const pane = document.createElement('div');
    pane.className = 'term-pane hidden';
    pane.dataset.paneId = id;
    this.panesEl.appendChild(pane);

    session.attach(pane);
    if (session.backend.greet) session.backend.greet();
    this.activate(id);
    return session;
  }

  activate(id) {
    this.activeId = id;
    for (const t of this.tabsEl.querySelectorAll('.term-tab')) {
      t.classList.toggle('active', t.dataset.tabId === id);
    }
    for (const p of this.panesEl.querySelectorAll('.term-pane')) {
      p.classList.toggle('hidden', p.dataset.paneId !== id);
    }
    const s = this.sessions.get(id);
    if (s && s.term) s.term.focus();
  }

  closeTab(id) {
    const s = this.sessions.get(id);
    if (s) s.close();
    this.sessions.delete(id);
    this.tabsEl.querySelector(`[data-tab-id="${id}"]`)?.remove();
    this.panesEl.querySelector(`[data-pane-id="${id}"]`)?.remove();
    const remaining = [...this.sessions.keys()];
    if (remaining.length > 0) this.activate(remaining[remaining.length - 1]);
    else this.activeId = null;
  }
}

let _termManager = null;
function initTerminal() {
  if (_termManager) return _termManager;
  const tabsEl = document.getElementById('term-tabs');
  const panesEl = document.getElementById('term-panes');
  if (!tabsEl || !panesEl) return null;
  _termManager = new TerminalManager(tabsEl, panesEl);
  document.getElementById('term-new-tab')?.addEventListener('click', () => _termManager.newTab());
  _termManager.newTab();
  return _termManager;
}

if (typeof module !== 'undefined') module.exports = { TerminalSession, FakeShell, TerminalManager, TERMINAL_DEMO_MODE };

function showTerminalTab() {
  document.getElementById("messages").classList.add("hidden");
  document.getElementById("composer").classList.add("hidden");
  const tp = document.getElementById("timeline-panel");
  if (tp) tp.classList.add("hidden");
  const np = document.getElementById("network-panel");
  if (np) np.classList.add("hidden");
  document.getElementById("terminal-view").classList.remove("hidden");
  document.getElementById("tab-chat").classList.remove("active");
  const tt = document.getElementById("tab-timeline");
  if (tt) tt.classList.remove("active");
  const nt = document.getElementById("tab-network");
  if (nt) nt.classList.remove("active");
  document.getElementById("tab-terminal").classList.add("active");
  if (typeof initTerminal === "function") initTerminal();
}

function hideTerminalTab() {
  document.getElementById("terminal-view").classList.add("hidden");
  document.getElementById("tab-terminal").classList.remove("active");
}
