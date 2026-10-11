// Spanish for the landing page. The English in index.html is the source: each text node,
// aria-label, alt and meta whose English text is a key below gets the Spanish instead.
// Clips and settings crops switch to their Spanish recordings in media/es/.
// The language comes from ?lang=, then the visitor's last pick, then the browser's languages.
// Wording follows the app's own Spanish (gui/src/shared/i18n/es.ts).

const ES = {
  // Head
  'Worldline: game save backups with rewind and branching': 'Worldline: copias de tus partidas guardadas, con vuelta atrás y ramas',
  'Automatic backups for your PC game saves. Rewind a bad call, branch into the road not taken, and sync across devices. Free and open source, Windows & Linux.':
    'Copias automáticas de las partidas guardadas de tus juegos de PC. Deshaz una mala decisión, toma el camino que no tomaste y sincroniza entre dispositivos. Libre y de código abierto, Windows y Linux.',
  'Worldline: every save, every timeline': 'Worldline: cada partida, cada línea temporal',
  'Automatic checkpoints for your game saves, with rewind, branching and sync across your devices. Free and open source.':
    'Puntos de control automáticos de tus partidas guardadas, con vuelta atrás, ramas y sincronización entre tus dispositivos. Libre y de código abierto.',

  // Title bar
  'Worldline home': 'Inicio de Worldline',
  'See it': 'Míralo',
  'How it works': 'Cómo funciona',
  'In game': 'En el juego',
  'Try it': 'Pruébalo',
  'Worldline on GitHub': 'Worldline en GitHub',
  'Download': 'Descargar',
  'Language': 'Idioma',

  // Hero
  'Automatic save backups for PC games': 'Copias automáticas de partidas para juegos de PC',
  'Every save. Every timeline.': 'Cada partida. Cada línea temporal.',
  'Worldline keeps a checkpoint every time your game saves. Rewind a bad call, take the road not taken, and pick up on your other PC right where you left off.':
    'Worldline guarda un punto de control cada vez que tu juego guarda. Deshaz una mala decisión, toma el camino que no tomaste y sigue en tu otra PC justo donde lo&nbsp;dejaste.',
  'Download for Windows': 'Descargar para Windows',
  'Download for Linux': 'Descargar para Linux',
  'Latest release': 'Última versión',
  'For Windows and Linux': 'Para Windows y Linux',
  'See it in action': 'Míralo en acción',
  'Windows & Linux · Free and open source, MIT ·': 'Windows y Linux · Libre y de código abierto, MIT ·',
  'All downloads': 'Todas las descargas',

  // See it
  'Your saves, as a timeline.': 'Tus partidas, como una línea temporal.',
  'Pick a game and see every time you played it, with a checkpoint for each save. Switch to the graph to see every timeline at once.':
    'Elige un juego y mira cada vez que lo jugaste, con un punto de control por cada guardado. Cambia al grafo para ver todas las líneas temporales a la&nbsp;vez.',
  "Worldline showing a game's checkpoints grouped into sessions, then switching to the graph of its timelines":
    'Worldline muestra los puntos de control de un juego agrupados en sesiones y luego cambia al grafo de sus líneas temporales',
  'Recorded from Worldline, with demo saves': 'Grabado de Worldline, con partidas de prueba',

  // How it works
  'Install it. Then forget about it.': 'Instálalo. Y olvídate.',
  'Every save becomes a checkpoint.': 'Cada guardado se vuelve un punto de control.',
  'When a game saves, Worldline keeps a copy. Checkpoints group into the sessions you actually played, and naming one keeps it forever.':
    'Cuando un juego guarda, Worldline conserva una copia. Los puntos de control se agrupan en las sesiones que de verdad jugaste, y ponerle nombre a uno lo conserva para siempre.',
  'Naming the latest Hollow Knight checkpoint Hornet defeated': 'Se le pone nombre al último punto de control de Hollow Knight: Hornet derrotada',
  'Hold to rewind.': 'Mantén para volver.',
  'Pick a checkpoint and hold the button. The save you’re leaving isn’t thrown away: it stays as its own timeline, so you can always go back to it.':
    'Elige un punto de control y mantén el botón. La partida que dejas no se tira: queda como su propia línea temporal, así que siempre puedes regresar a ella.',
  'Selecting the Before Hornet checkpoint and holding to rewind to it': 'Se elige el punto de control Before Hornet y se mantiene el botón para volver a él',
  'Pick up on another PC.': 'Sigue en otra PC.',
  'Point Worldline at a folder you already sync, like Dropbox or Syncthing. Played on your laptop? It shows up at the top. Continue here. If both PCs played, both timelines are kept and you choose.':
    'Indícale a Worldline una carpeta que ya sincronices, como Dropbox o Syncthing. ¿Jugaste en tu portátil? Aparece arriba: pulsa Seguir aquí. Si jugaron las dos PC, se conservan ambas líneas temporales y tú eliges.',
  'A newer save from the laptop appears at the top, and Continue here brings it in': 'Arriba aparece una partida más nueva del portátil, y Seguir aquí la trae',
  'Finds your games.': 'Encuentra tus juegos.',
  'Scans Steam, Epic and your own folders, and knows where thousands of games keep their saves thanks to the Ludusavi manifest. Games you track on your other PCs show up too.':
    'Busca en Steam, Epic y tus propias carpetas, y sabe dónde guardan sus partidas miles de juegos gracias al manifiesto de Ludusavi. También aparecen los juegos que sigues en tus otras PC.',
  'Looking for installed games and adding two of them': 'Se buscan los juegos instalados y se agregan dos',

  // In the game
  'In the game': 'En el juego',
  'It tells you without pausing you.': 'Te avisa sin pausarte.',
  'When Worldline keeps a checkpoint, a small popup slides into a corner of the game and gets out of the way. Press a hotkey to keep the moment for good, or to name it right over the game.':
    'Cuando Worldline guarda un punto de control, un pequeño aviso entra por una esquina del juego y se aparta. Pulsa un atajo para conservar el momento para siempre, o para ponerle nombre sobre el juego.',
  'Overlay clips': 'Clips del aviso en el juego',
  'Popups': 'Avisos',
  'Name it hotkey': 'Atajo para ponerle nombre',
  'Worldline popups in the corner of a game: recording, checkpoint 42 then 43, a new timeline, kept forever':
    'Avisos de Worldline en la esquina de un juego: grabando, punto de control 42 y luego 43, una nueva línea temporal, conservado para siempre',
  'The Name it hotkey opens a card over the game; typing Spared the mimic and pressing Enter keeps it':
    'El atajo para ponerle nombre abre una tarjeta sobre el juego; se escribe Perdoné al mímico y Enter lo conserva',
  'Popup: Checkpoint #42, Lantern Knight': 'Aviso: Punto de control #42, Lantern Knight',
  'Popup: New timeline #44, branched off “Before the bridge”': 'Aviso: Nueva línea temporal #44, se separa de «Antes del puente»',
  'Popup: Kept forever #46, “Spared the mimic”': 'Aviso: Conservado para siempre #46, «Perdoné al mímico»',
  'Popup: Laptop is ahead, 3 newer checkpoints, quit and sync first': 'Aviso: Portátil va por delante, 3 puntos de control más nuevos, sal y sincroniza primero',
  'Every checkpoint': 'Cada punto de control',
  'When you branch off': 'Cuando te separas',
  'Kept with a hotkey': 'Conservado con un atajo',
  'When another PC is ahead': 'Cuando otra PC va por delante',
  'The real popups, over a stand-in game. Works in windowed and borderless games.':
    'Los avisos reales, sobre un juego de ejemplo. Funciona en juegos en ventana y en ventana sin bordes.',

  // Try it
  'Play a tiny game, then rewind it.': 'Juega un juego diminuto y vuelve atrás.',
  'A thirty-second toy version: play, lose your gold, rewind, take the other road.':
    'Una versión de juguete de treinta segundos: juega, pierde tu oro, vuelve atrás, toma el otro camino.',
  'Tour steps': 'Pasos del recorrido',
  'Checkpoints': 'Puntos de control',
  'Rewind': 'Volver',
  'Timelines': 'Líneas temporales',
  'Just play.': 'Solo juega.',
  'Every time the game saves, Worldline keeps a checkpoint. You never press a thing.':
    'Cada vez que el juego guarda, Worldline guarda un punto de control. No tienes que pulsar nada.',
  'Made a bad call?': '¿Una mala decisión?',
  'There’s a chest up ahead. Surely it’s fine.': 'Hay un cofre más adelante. Seguro que no pasa nada.',
  'A mimic! Your gold is gone, and the game already autosaved over it. Hold to go back.':
    '¡Un mímico! Tu oro desapareció y el juego ya se autoguardó encima. Mantén para volver.',
  'Take the other path.': 'Toma el otro camino.',
  'Back before the chest, gold intact. The mimic run is still kept, just in case. Now walk past it.':
    'De vuelta antes del cofre, con el oro intacto. La partida del mímico sigue guardada, por si acaso. Ahora pasa de largo.',
  'That’s a worldline.': 'Eso es una worldline.',
  'Your line, plus every road not taken. Nothing ever gets overwritten.':
    'Tu línea, más cada camino que no tomaste. Nunca se sobrescribe nada.',
  'Play': 'Jugar',
  'Open the chest': 'Abrir el cofre',
  'Walk past it': 'Pasar de largo',
  'Hold to rewind': 'Mantén para volver',
  'Get Worldline': 'Consigue Worldline',
  'Replay': 'Repetir',
  'Saving…': 'Guardando…',
  'Village': 'Aldea',
  'Forest': 'Bosque',
  'Old chest': 'Cofre viejo',
  'Mimic': 'Mímico',
  'Castle gate': 'Puerta del castillo',
  'Your timeline': 'Tu línea temporal',
  '◀◀ REWIND': '◀◀ VOLVER',

  // Small stuff
  'And the small stuff': 'Y los detalles',
  'Fewer worries. More attempts.': 'Menos preocupaciones. Más intentos.',
  'Worldline settings: Quick keep hotkey Ctrl Shift K and Name it hotkey Ctrl Shift N':
    'Ajustes de Worldline: atajo para conservar rápido Ctrl Shift K y atajo para ponerle nombre Ctrl Shift N',
  'Set it up your way.': 'Configúralo a tu manera.',
  'Pick the popup’s corner for each game, choose your hotkeys, or switch to Quiet mode: only the Worldline mark, for streams and speedruns.':
    'Elige la esquina del aviso para cada juego, elige tus atajos o cambia al modo discreto: solo la marca de Worldline, para directos y speedruns.',
  'Worldline settings: keep everything for 1 hour, then hourly up to 1 day, daily up to 7 days, weekly up to 3 months':
    'Ajustes de Worldline: conservar todo durante 1 hora, luego uno por hora hasta 1 día, uno por día hasta 7 días y uno por semana hasta 3 meses',
  'Tidies up after itself.': 'Ordena por su cuenta.',
  'Everything for the first hour, then hourly, daily and weekly. Named checkpoints never go, and you choose the steps.':
    'Todo durante la primera hora, luego uno por hora, por día y por semana. Los puntos de control con nombre nunca se van, y tú eliges los pasos.',
  'Recording': 'Grabando',
  'New from Laptop': 'Nuevo del portátil',
  'Last week': 'La semana pasada',
  'Not in the cloud yet': 'Aún no está en la nube',
  'Every game gets its own color.': 'Cada juego tiene su propio color.',
  'The whole app takes on the color of the game you pick.': 'Toda la app toma el color del juego que eliges.',
  'Go on, pick one.': 'Anda, elige uno.',
  'Worldline settings: wait 3 seconds after the game saves, at most one checkpoint every 5 minutes, screenshot each checkpoint':
    'Ajustes de Worldline: esperar 3 segundos después de que el juego guarda, como mucho un punto de control cada 5 minutos, captura de pantalla con cada punto de control',
  'Gentle with your saves.': 'Cuidadoso con tus partidas.',
  'Waits for the game to finish writing, turns quick saves in a row into one checkpoint, and can keep a screenshot with each.':
    'Espera a que el juego termine de escribir, junta varios guardados seguidos en un punto de control y puede guardar una captura de pantalla con cada uno.',
  'No lock-in.': 'Sin ataduras.',
  'Plain files on your disk, checksummed. Rebuild any save by hand with zstd, or export a game’s whole history as a .zip.':
    'Archivos comunes en tu disco, con suma de verificación. Reconstruye cualquier partida a mano con zstd, o exporta todo el historial de un juego como .zip.',
  'Terminal, too.': 'También en la terminal.',
  'The desktop app and the CLI share one history. List games, export and import history, sync, clean up, check every stored chunk.':
    'La app de escritorio y la CLI comparten un mismo historial. Lista juegos, exporta e importa historial, sincroniza, limpia y verifica cada fragmento guardado.',
  'Example CLI commands': 'Comandos de ejemplo de la CLI',

  // Outro, footer, star prompt
  'Start keeping every save.': 'Empieza a conservar cada partida.',
  'Free, open source, and it stays out of your way.': 'Gratis, de código abierto y no te estorba.',
  'Star it on GitHub': 'Dale una estrella en GitHub',
  'MIT licensed · Made by Agustín Morantes': 'Licencia MIT · Hecho por Agustín Morantes',
  'Star Worldline on GitHub': 'Dale una estrella a Worldline en GitHub',
  'Like what you see?': '¿Te gusta?',
  'A star helps other players find Worldline.': 'Una estrella ayuda a que otros jugadores encuentren Worldline.',
  'Star on GitHub': 'Estrella en GitHub',
  'Dismiss': 'Cerrar',
};

const WL = (() => {
  const LANGS = ['en', 'es'];
  const KEY = 'wl-lang';
  const norm = (s) => s.replace(/\s+/g, ' ').trim();
  const orig = new WeakMap(); // node or attribute owner -> its English text
  const ATTRS = ['aria-label', 'alt', 'content'];
  let lang = 'en';
  const listeners = [];

  function detect() {
    const q = new URLSearchParams(location.search).get('lang');
    if (LANGS.includes(q)) return q;
    try {
      const saved = localStorage.getItem(KEY);
      if (LANGS.includes(saved)) return saved;
    } catch {}
    for (const l of navigator.languages || [navigator.language]) {
      const base = (l || '').slice(0, 2).toLowerCase();
      if (LANGS.includes(base)) return base;
    }
    return 'en';
  }

  const t = (en) => (lang === 'es' && ES[en] !== undefined ? ES[en] : en);

  function translateText(node) {
    let en = orig.get(node);
    if (en === undefined) {
      en = norm(node.nodeValue);
      if (ES[en] === undefined) return;
      orig.set(node, en);
    }
    const lead = node.nodeValue.match(/^\s*/)[0];
    const trail = node.nodeValue.match(/\s*$/)[0];
    // Spanish strings may carry &nbsp; to keep the last two words together.
    node.nodeValue = lead + t(en).replace(/&nbsp;/g, ' ') + trail;
  }

  function translateAttrs(el) {
    let saved = orig.get(el);
    if (!saved) {
      saved = {};
      for (const a of ATTRS) {
        const v = el.getAttribute(a);
        if (v && ES[norm(v)] !== undefined) saved[a] = norm(v);
      }
      if (!Object.keys(saved).length) return;
      orig.set(el, saved);
    }
    for (const [a, en] of Object.entries(saved)) el.setAttribute(a, t(en));
  }

  function apply() {
    document.documentElement.lang = lang;
    const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
    const nodes = [];
    while (walker.nextNode()) nodes.push(walker.currentNode);
    nodes.forEach(translateText);
    document.querySelectorAll('[aria-label], img[alt], meta[content]').forEach(translateAttrs);
    if (!orig.has(document)) orig.set(document, document.title);
    document.title = t(orig.get(document));
    document.querySelectorAll('[data-lang]').forEach((b) => b.setAttribute('aria-pressed', String(b.dataset.lang === lang)));
    localizeMedia();
  }

  // Clips and stills recorded from the Spanish app live in media/es/ under the same names.
  const mediaFor = (url) => (lang === 'es' ? url.replace(/^media\/(?!es\/|icons\/)/, 'media/es/') : url.replace(/^media\/es\//, 'media/'));
  function localizeMedia() {
    document.querySelectorAll('video[src^="media/"], img[src^="media/"]').forEach((m) => {
      const src = mediaFor(m.getAttribute('src'));
      if (src === m.getAttribute('src')) return;
      const playing = m.tagName === 'VIDEO' && !m.paused;
      if (m.hasAttribute('poster')) m.setAttribute('poster', mediaFor(m.getAttribute('poster')));
      m.setAttribute('src', src);
      if (playing) m.play().catch(() => {});
    });
  }

  function set(next, { save = true } = {}) {
    if (!LANGS.includes(next)) return;
    lang = next;
    if (save) try { localStorage.setItem(KEY, next); } catch {}
    // Keep a shared ?lang= link in step with the pick, so a reload doesn't flip it back.
    const url = new URL(location.href);
    if (url.searchParams.has('lang')) {
      url.searchParams.set('lang', next);
      history.replaceState(history.state, '', url);
    }
    apply();
    listeners.forEach((fn) => fn(lang));
  }

  lang = detect();
  apply();
  document.querySelectorAll('[data-lang]').forEach((b) => b.addEventListener('click', () => set(b.dataset.lang)));

  return { t, get lang() { return lang; }, onChange: (fn) => listeners.push(fn) };
})();
