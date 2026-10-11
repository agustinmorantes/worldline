// Worldline landing page: latest-release download links, the hero icon pulse,
// the app clips, the overlay tabs, the playable demo, the Library re-tint and scroll reveals.

const REPO = 'agustinmorantes/worldline';
const reduced = matchMedia('(prefers-reduced-motion: reduce)').matches;
const $ = (s, el = document) => el.querySelector(s);
const $$ = (s, el = document) => [...el.querySelectorAll(s)];

/* ---------- Download links ---------- */

function detectOS() {
  const p = (navigator.userAgentData?.platform || navigator.platform || navigator.userAgent).toLowerCase();
  if (p.includes('win')) return 'windows';
  if (p.includes('linux') || p.includes('x11') || p.includes('cros')) return 'linux';
  return 'other';
}

async function wireDownloads() {
  const os = detectOS();
  const label = os === 'linux' ? 'Download for Linux' : os === 'windows' ? 'Download for Windows' : 'Download';
  $$('[data-dl-label]').forEach((el) => (el.textContent = label));
  if (os === 'other') $$('[data-dl-meta]').forEach((el) => (el.textContent = 'For Windows and Linux'));

  let release;
  try {
    const res = await fetch(`https://api.github.com/repos/${REPO}/releases/latest`, {
      headers: { Accept: 'application/vnd.github+json' },
    });
    if (!res.ok) return; // not public or no release yet: links keep pointing at /releases/latest
    release = await res.json();
  } catch {
    return;
  }

  const assets = release.assets || [];
  const find = (re) => assets.find((a) => re.test(a.name));
  const asset =
    os === 'linux' ? find(/\.AppImage$/i) || find(/\.deb$/i) || find(/\.rpm$/i)
    : os === 'windows' ? find(/setup\.exe$/i) || find(/\.msi$/i) || find(/\.exe$/i)
    : null;

  const version = release.tag_name || release.name || 'Latest';
  const meta = asset ? `${version} · ${Math.max(1, Math.round(asset.size / 1048576))} MB` : version;
  $$('[data-dl-meta]').forEach((el) => (el.textContent = meta));
  if (asset) $$('[data-dl]').forEach((el) => (el.href = asset.browser_download_url));
}

/* ---------- Hero icon: a pulse runs down the line and picks a branch ---------- */

function animateIcon() {
  const card = $('#card');
  if (!card || reduced) return;
  let n = 0;
  let to;
  const cycle = () => {
    n += 1;
    const side = n % 2 ? 'L' : 'R';
    card.classList.remove('cycA', 'cycB', 'cycX');
    void card.getBoundingClientRect(); // restart the CSS animations
    card.classList.add(n % 2 ? 'cycA' : 'cycB');
    clearTimeout(to);
    to = setTimeout(() => {
      card.classList.remove('litL', 'litR');
      card.classList.add(`lit${side}`);
    }, 1250);
  };
  setTimeout(cycle, 900);
  setInterval(cycle, 3200);
}

/* ---------- App clips: play while on screen ---------- */

// Clips load nothing until they come near the viewport and pause when they leave it.
// With reduced motion they stay on their poster and get controls instead.
const clipIO = 'IntersectionObserver' in window && !reduced
  ? new IntersectionObserver((entries) => {
    for (const e of entries) {
      if (e.isIntersecting) e.target.play().catch(() => {});
      else e.target.pause();
    }
  }, { rootMargin: '200px 0px' })
  : null;

function watchClip(v) {
  if (clipIO) clipIO.observe(v);
  else {
    v.controls = true;
    v.preload = 'metadata';
  }
}

function wireClips() {
  $$('video[data-clip]').forEach(watchClip);
}

/* ---------- Overlay: Popups / Name it tabs ---------- */

function wireOverlayTabs() {
  const tabs = $$('[data-ov]');
  tabs.forEach((tab) => tab.addEventListener('click', () => {
    tabs.forEach((t) => {
      const on = t === tab;
      const v = $(`#ov-${t.dataset.ov}`);
      t.setAttribute('aria-selected', String(on));
      v.classList.toggle('on', on);
      if (on) {
        v.currentTime = 0; // start the clip being shown from the top
        watchClip(v);
        if (clipIO) v.play().catch(() => {});
      } else {
        clipIO?.unobserve(v);
        v.pause();
      }
    });
  }));
}

/* ---------- Library: picking a game re-tints the page ---------- */

function wireLibrary() {
  const lib = $('[data-lib]');
  if (!lib) return;
  const pill = $('[data-lib-pill]', lib);
  const pick = (row) => {
    document.documentElement.style.setProperty('--h', row.dataset.hue);
    pill.style.transform = `translateY(${row.offsetTop - 10}px)`;
    pill.classList.add('on');
    $$('.lib-row', lib).forEach((r) => r.setAttribute('aria-pressed', String(r === row)));
  };
  $$('.lib-row', lib).forEach((row) => {
    row.addEventListener('click', () => pick(row));
    row.addEventListener('pointerenter', () => pick(row));
    row.addEventListener('focus', () => pick(row));
  });
}

/* ---------- Demo: Lantern Knight ---------- */

const CPS = {
  c1: { x: 14 },
  c2: { x: 36 },
  c3: { x: 58 },
  c4: { x: 70, bad: true },
  c5: { x: 86 },
};
const LANE_Y = [40, 92];

const SNAP = {
  1: { step: 1, phase: 'play', knight: 4, mimic: false, gold: 120, cps: [], hollow: false, branched: false, token: null },
  2: { step: 2, phase: 'open', knight: 58, mimic: false, gold: 120, cps: ['c1', 'c2', 'c3'], hollow: false, branched: false, token: 'c3' },
  3: { step: 3, phase: 'walk', knight: 58, mimic: false, gold: 120, cps: ['c1', 'c2', 'c3', 'c4'], hollow: true, branched: false, token: 'c3' },
  4: { step: 4, phase: 'done', knight: 86, mimic: false, gold: 120, cps: ['c1', 'c2', 'c3', 'c4', 'c5'], hollow: true, branched: true, token: 'c5' },
};

const COPY = {
  play: ['Just play.', 'Every time the game saves, Worldline keeps a checkpoint. You never press a thing.'],
  open: ['Made a bad call?', 'There’s a chest up ahead. Surely it’s fine.'],
  rewind: ['Made a bad call?', 'A mimic! Your gold is gone, and the game already autosaved over it. Hold to go back.'],
  walk: ['Take the other path.', 'Back before the chest, gold intact. The mimic run is still kept, just in case. Now walk past it.'],
  done: ['That’s a worldline.', 'Your line, plus every road not taken. Nothing ever gets overwritten.'],
};

function initDemo() {
  const stage = $('[data-stage]');
  if (!stage) return;
  const el = {
    scene: $('[data-scene]'),
    knight: $('[data-knight]'),
    toast: $('[data-toast]'),
    chest: $('[data-chest]'),
    gold: $('[data-gold]'),
    goldNum: $('[data-gold] b'),
    strip: $('[data-strip]'),
    fill: $('[data-fill]'),
    token: $('[data-token]'),
    text: $('.demo-text'),
    title: $('[data-d-title]'),
    body: $('[data-d-body]'),
    act: $('[data-d-act]'),
    hold: $('[data-d-hold]'),
    dl: $('[data-d-dl]'),
    replay: $('[data-d-replay]'),
    tabs: $$('[data-step]'),
    cps: Object.fromEntries($$('[data-cp]').map((n) => [n.dataset.cp, n])),
  };

  let s = structuredClone(SNAP[1]);
  let run = 0;
  let busy = false;
  const CANCEL = Symbol('cancel');
  const speed = reduced ? 0.15 : 1;

  const cpPos = (id) => {
    const lane = id === 'c4' && s.branched ? 1 : 0;
    return { x: lane ? 72 : CPS[id].x, y: LANE_Y[lane], lane };
  };

  function setText(phase) {
    const [t, b] = COPY[phase];
    if (el.title.textContent === t && el.body.textContent === b) return;
    el.title.textContent = t;
    el.body.textContent = b;
    el.text.classList.remove('swap');
    void el.text.offsetWidth;
    el.text.classList.add('swap');
  }

  function render() {
    el.knight.style.left = `${s.knight}%`;
    el.chest.classList.toggle('mimic', s.mimic);
    el.goldNum.textContent = String(s.gold);
    el.gold.classList.toggle('broke', s.gold === 0);

    for (const [id, node] of Object.entries(el.cps)) {
      const on = s.cps.includes(id);
      const p = cpPos(id);
      node.classList.toggle('on', on);
      node.classList.toggle('bad', !!CPS[id].bad && !s.hollow);
      node.classList.toggle('hollow', !!CPS[id].bad && s.hollow && !p.lane);
      node.classList.toggle('lane', p.lane === 1);
      node.classList.toggle('here', s.token === id);
      node.style.left = `${p.x}%`;
      node.style.top = `${p.y}px`;
    }
    el.strip.classList.toggle('branched', s.branched);

    const tx = s.token ? cpPos(s.token).x : 4;
    el.token.classList.toggle('on', !!s.token);
    el.token.style.left = `${tx}%`;
    el.fill.style.width = `${tx - 4}%`;

    setText(s.phase);
    el.tabs.forEach((t) => {
      const n = Number(t.dataset.step);
      t.setAttribute('aria-selected', String(n === Math.min(s.step, 3)));
      t.classList.toggle('done', n < s.step);
    });

    const actLabel = { play: 'Play', open: 'Open the chest', walk: 'Walk past it' }[s.phase];
    el.act.hidden = !actLabel;
    if (actLabel) el.act.textContent = actLabel;
    el.act.disabled = busy;
    el.hold.hidden = s.phase !== 'rewind';
    el.hold.disabled = busy;
    el.dl.hidden = el.replay.hidden = s.phase !== 'done';
  }

  // Runs one animated action; a step jump or replay cancels it.
  function task(fn) {
    const my = ++run;
    const wait = (ms) => new Promise((r) => setTimeout(r, ms * speed)).then(() => {
      if (my !== run) throw CANCEL;
    });
    busy = true;
    render();
    fn(wait)
      .catch((e) => { if (e !== CANCEL) throw e; })
      .finally(() => {
        if (my === run) { busy = false; render(); }
      });
  }

  function jump(step) {
    run += 1;
    busy = false;
    el.scene.classList.remove('rewinding', 'shake');
    el.knight.classList.remove('walking', 'hurt');
    el.toast.classList.remove('on');
    $$('.drop', stage).forEach((d) => d.remove());
    el.knight.style.transitionDuration = '400ms';
    s = structuredClone(SNAP[step]);
    render();
  }

  async function walkTo(x, wait, { walk = true } = {}) {
    const dist = Math.abs(x - s.knight);
    const ms = dist * 38;
    el.knight.style.transitionDuration = `${ms * speed}ms`;
    el.knight.classList.toggle('walking', walk);
    s.knight = x;
    render();
    await wait(ms);
    el.knight.classList.remove('walking');
  }

  async function save(id, wait) {
    el.toast.classList.add('on');
    await wait(520);
    el.toast.classList.remove('on');

    // A dot drops from the game onto the timeline.
    const sr = stage.getBoundingClientRect();
    const kr = el.knight.getBoundingClientRect();
    const tr = el.strip.getBoundingClientRect();
    const x0 = kr.left + kr.width / 2 - sr.left;
    const y0 = kr.top - sr.top + 10;
    const x1 = tr.left - sr.left + (tr.width * CPS[id].x) / 100;
    const y1 = tr.top - sr.top + LANE_Y[0];
    const drop = document.createElement('span');
    drop.className = `drop${CPS[id].bad ? ' bad' : ''}`;
    stage.append(drop);
    if (!reduced) {
      await drop.animate(
        [
          { transform: `translate(${x0}px, ${y0}px) scale(.6)` },
          { transform: `translate(${(x0 + x1) / 2}px, ${y0 - 26}px) scale(1.1)`, offset: 0.3 },
          { transform: `translate(${x1}px, ${y1}px) scale(1)` },
        ],
        { duration: 560, easing: 'cubic-bezier(.45,0,.8,.6)' },
      ).finished.catch(() => {});
    }
    drop.remove();
    await wait(0);

    s.cps = [...s.cps, id];
    s.token = id;
    const node = el.cps[id];
    node.classList.remove('pop');
    void node.offsetWidth;
    node.classList.add('pop');
    render();
    await wait(260);
  }

  async function countGold(to, wait) {
    const from = s.gold;
    const steps = 8;
    for (let i = 1; i <= steps; i++) {
      s.gold = Math.round(from + ((to - from) * i) / steps);
      el.goldNum.textContent = String(s.gold);
      await wait(40);
    }
    render();
  }

  const actions = {
    play: () => task(async (wait) => {
      for (const id of ['c1', 'c2', 'c3']) {
        await walkTo(CPS[id].x, wait);
        await save(id, wait);
      }
      await wait(300);
      Object.assign(s, { step: 2, phase: 'open' });
    }),

    open: () => task(async (wait) => {
      await walkTo(60, wait);
      s.mimic = true;
      render();
      await wait(380);
      el.scene.classList.add('shake');
      el.knight.classList.add('hurt');
      await countGold(0, wait);
      await wait(300);
      el.scene.classList.remove('shake');
      el.knight.classList.remove('hurt');
      await save('c4', wait);
      s.phase = 'rewind';
    }),

    rewind: () => task(async (wait) => {
      el.scene.classList.add('rewinding');
      s.token = 'c3';
      s.mimic = false;
      render();
      await Promise.all([countGold(120, wait), walkTo(58, wait, { walk: false })]);
      await wait(700);
      el.scene.classList.remove('rewinding');
      Object.assign(s, { hollow: true, step: 3, phase: 'walk' });
    }),

    walk: () => task(async (wait) => {
      s.branched = true;
      render();
      await wait(650);
      await walkTo(CPS.c5.x, wait);
      await save('c5', wait);
      Object.assign(s, { step: 4, phase: 'done' });
      document.dispatchEvent(new Event('demo-done'));
    }),
  };

  el.act.addEventListener('click', () => !busy && actions[s.phase]?.());
  el.replay.addEventListener('click', () => jump(1));
  el.tabs.forEach((t) => t.addEventListener('click', () => jump(Number(t.dataset.step))));

  // Hold to rewind: 900ms, pointer or Space/Enter, like the app.
  let holdTimer;
  const startHold = () => {
    if (busy || s.phase !== 'rewind' || holdTimer) return;
    el.hold.classList.add('holding');
    holdTimer = setTimeout(() => {
      holdTimer = null;
      el.hold.classList.remove('holding');
      actions.rewind();
    }, 900);
  };
  const endHold = () => {
    clearTimeout(holdTimer);
    holdTimer = null;
    el.hold.classList.remove('holding');
  };
  el.hold.addEventListener('pointerdown', (e) => { e.preventDefault(); startHold(); });
  ['pointerup', 'pointerleave', 'pointercancel'].forEach((ev) => el.hold.addEventListener(ev, endHold));
  el.hold.addEventListener('keydown', (e) => {
    if ((e.key === ' ' || e.key === 'Enter') && !e.repeat) { e.preventDefault(); startHold(); }
  });
  el.hold.addEventListener('keyup', (e) => { if (e.key === ' ' || e.key === 'Enter') endHold(); });
  el.hold.addEventListener('click', (e) => e.preventDefault());

  render();
}

/* ---------- Scroll reveals ---------- */

function reveals() {
  const targets = $$('.sec-head, .appshot, .row-copy, .row .clip, .gameshot, .demo, .cell, .outro');
  if (reduced || !('IntersectionObserver' in window)) {
    targets.forEach((t) => t.classList.add('in'));
    return;
  }
  targets.forEach((t) => t.classList.add('reveal'));
  const io = new IntersectionObserver((entries) => {
    for (const e of entries) {
      if (!e.isIntersecting) continue;
      e.target.classList.add('in');
      setTimeout(() => (e.target.style.transitionDelay = ''), 900);
      io.unobserve(e.target);
    }
  }, { threshold: 0.15 });
  targets.forEach((t, i) => {
    t.style.transitionDelay = t.classList.contains('cell') ? `${(i % 3) * 70}ms` : '';
    io.observe(t);
  });
}

/* ---------- Stars ---------- */

const fmtStars = (n) => (n < 1000 ? String(n) : `${(n / 1000).toFixed(n < 10000 ? 1 : 0).replace(/\.0$/, '')}k`);

async function wireStars() {
  try {
    const res = await fetch(`https://api.github.com/repos/${REPO}`, {
      headers: { Accept: 'application/vnd.github+json' },
    });
    if (!res.ok) return; // private or rate-limited: the badges stay hidden
    const n = (await res.json()).stargazers_count;
    if (typeof n !== 'number') return;
    $$('[data-stars]').forEach((el) => {
      el.querySelector('b').textContent = fmtStars(n);
      el.hidden = false;
      if (!reduced) el.classList.add('bump');
    });
  } catch {}
}

// A one-time nudge once the visitor has seen the demo through or reached the end of the page.
function starPrompt() {
  const KEY = 'wl-star-ask';
  const ask = $('[data-star-ask]');
  try {
    if (localStorage.getItem(KEY)) return;
  } catch {}

  let shown = false;
  const show = () => {
    if (shown) return;
    shown = true;
    setTimeout(() => (ask.hidden = false), 1200);
  };
  const dismiss = () => {
    try { localStorage.setItem(KEY, '1'); } catch {}
    if (reduced) return void (ask.hidden = true);
    ask.classList.add('out');
    ask.addEventListener('animationend', () => (ask.hidden = true), { once: true });
  };

  document.addEventListener('demo-done', show, { once: true });
  if ('IntersectionObserver' in window) {
    const io = new IntersectionObserver((entries) => {
      if (entries.some((e) => e.isIntersecting)) { io.disconnect(); show(); }
    }, { threshold: 0.6 });
    io.observe($('.outro'));
  }

  $('[data-star-close]').addEventListener('click', dismiss);
  $('[data-star-go]').addEventListener('click', dismiss);
  document.addEventListener('keydown', (e) => e.key === 'Escape' && !ask.hidden && dismiss());
}

wireDownloads();
wireStars();
starPrompt();
animateIcon();
wireClips();
wireOverlayTabs();
wireLibrary();
initDemo();
reveals();
