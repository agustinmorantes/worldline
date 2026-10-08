import type { Messages } from './en';
import { plural } from './plural';

export const es: Messages = {
  common: {
    checkpoints: (count) => plural('es', count, { one: '# punto de control', other: '# puntos de control' }),
  },
  errors: {
    loadFailed: 'No se pudo cargar. Inténtalo de nuevo en un momento.',
    actionFailed: 'No funcionó. Inténtalo de nuevo.',
    detectionFailed: 'No se pudo buscar partidas nuevas.',
    pruningFailed: 'Falló la limpieza automática del historial.',
    copyDetails: 'Copiar detalles',
  },
  shell: {
    library: 'Biblioteca',
    addGames: 'Añadir juegos',
    settings: 'Ajustes',
    inCloudBadge: (count) =>
      plural('es', count, { one: '# juego de tu nube aún no está en este PC', other: '# juegos de tu nube aún no están en este PC' }),
    help: 'Ayuda',
    replayTour: 'Repetir el recorrido',
    minimize: 'Minimizar',
    maximize: 'Maximizar',
    close: 'Cerrar',
  },
  gameStatus: {
    newFrom: (device) => `Novedades de ${device}`,
    playedToo: (device) => `También jugaste en ${device}`,
    notInCloud: 'Aún no está en la nube',
    noSaves: 'Sin partidas guardadas',
  },
  sync: {
    thisPc: 'Este PC',
    cloud: 'Nube',
    syncing: 'Sincronizando…',
    synced: (when) => `Sincronizado ${when}`,
    notSynced: 'Aún sin sincronizar',
    unreachable: 'No se pudo llegar a la nube',
    syncNow: 'Sincronizar ahora',
    failed: 'No se pudo sincronizar.',
    turnOn: 'Activar la nube',
    turnOnHint: 'Tus partidas en todos tus dispositivos',
  },
  notices: {
    oldDataMoved: (path) =>
      `Los datos de una versión preliminar se movieron a ${path} y la aplicación empezó de cero.`,
  },
};
