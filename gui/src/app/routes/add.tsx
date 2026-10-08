import { createFileRoute } from '@tanstack/react-router';
import { AddGamesView } from '@/features/add-games/AddGamesView';

export const Route = createFileRoute('/add')({ component: AddGamesView });
