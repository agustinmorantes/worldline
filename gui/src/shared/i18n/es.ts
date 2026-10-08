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
  notices: {
    oldDataMoved: (path) =>
      `Los datos de una versión preliminar se movieron a ${path} y la aplicación empezó de cero.`,
  },
};
