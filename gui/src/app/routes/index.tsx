import { createFileRoute } from '@tanstack/react-router';
import { Home } from '@/features/library/Home';

export const Route = createFileRoute('/')({ component: Home });
