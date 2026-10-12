// Saving a checkpoint's name from the game screen; an empty name removes it.
import { tagVersion } from '@/ipc/commands';
import { queryClient } from '@/app/query-client';
import { reportError } from '@/shared/errors/report';

/** Whether the name was saved; a failure is reported here and resolves `false`. */
export async function nameCheckpoint(gameId: number, versionId: number, tag: string, failed: string): Promise<boolean> {
  try {
    await tagVersion(versionId, tag);
    await queryClient.invalidateQueries({ queryKey: ['game', gameId] });
    return true;
  } catch (err) {
    reportError(failed, err);
    return false;
  }
}
