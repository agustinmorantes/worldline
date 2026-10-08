import { createFileRoute } from '@tanstack/react-router';
import type { ReactElement } from 'react';
import { AddGamesView } from '@/features/add-games/AddGamesView';

interface AddSearch {
  /** Start scanning this PC right away (the tour's and the empty library's "Find my games"). */
  scan?: boolean;
}

export const Route = createFileRoute('/add')({
  validateSearch: (search: Record<string, unknown>): AddSearch => ({ scan: search.scan === true ? true : undefined }),
  component: AddRoute,
});

function AddRoute(): ReactElement {
  const { scan } = Route.useSearch();
  return <AddGamesView scan={scan === true} />;
}
