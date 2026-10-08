// Builders for IPC payloads: unit tests and the `vite dev` mock fixtures.
import type { FullGame } from './bindings/FullGame';
import type { Slot } from './bindings/Slot';
import type { TrackedDir } from './bindings/TrackedDir';
import type { Version } from './bindings/Version';

const uuid = (prefix: number, id: number): string =>
  `${prefix.toString(16).padStart(8, '0')}-0000-4000-8000-${id.toString(16).padStart(12, '0')}`;

/** Naive UTC timestamp, as the backend serialises it. */
export const naive = (date: Date): string => date.toISOString().slice(0, -1);

export function makeVersion(overrides: Partial<Version> & Pick<Version, 'id'>): Version {
  const { id } = overrides;
  return {
    uuid: uuid(1, id),
    slot_id: 1,
    parent_id: null,
    version_number: id,
    folder_slug: `v${id.toString()}`,
    reason: 'change',
    tag: null,
    retired: false,
    created_at: naive(new Date(Date.UTC(2024, 0, id))),
    origin_device: null,
    ...overrides,
  };
}

export function makeSlot(overrides: Partial<Slot> & Pick<Slot, 'id'>): Slot {
  const { id } = overrides;
  const name = overrides.name ?? `Slot ${id.toString()}`;
  return {
    uuid: uuid(2, id),
    game_id: 1,
    name,
    name_normalized: name.toLowerCase(),
    folder_slug: name.toLowerCase().replace(/\W+/g, '-'),
    active_version_id: null,
    created_at: '2024-01-01T00:00:00',
    updated_at: '2024-01-01T00:00:00',
    ...overrides,
  };
}

export function makeTrackedDir(overrides: Partial<TrackedDir> & Pick<TrackedDir, 'id' | 'path'>): TrackedDir {
  return {
    uuid: uuid(3, overrides.id),
    game_id: 1,
    folder_slug: `dir-${overrides.id.toString()}`,
    label: null,
    exclusions: '[]',
    retired_at: null,
    ...overrides,
  };
}

export function makeGame(overrides: Partial<FullGame> & Pick<FullGame, 'id' | 'title'>): FullGame {
  const { id, title } = overrides;
  return {
    uuid: uuid(4, id),
    title_normalized: title.toLowerCase(),
    folder_slug: title.toLowerCase().replace(/\W+/g, '-'),
    active_slot_id: null,
    icon_path: null,
    accent_hue: 290,
    accent_hue_source: 'auto',
    tracked_dirs: [],
    slots: [],
    slot_summaries: [],
    active_slot: null,
    versions: [],
    last_version: null,
    ...overrides,
  };
}
