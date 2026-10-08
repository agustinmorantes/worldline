import { createFileRoute } from '@tanstack/react-router';
import { SettingsView } from '@/features/settings/SettingsView';

export const Route = createFileRoute('/settings')({ component: SettingsView });
