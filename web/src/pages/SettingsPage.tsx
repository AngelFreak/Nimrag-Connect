import { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { useQueryClient } from '@tanstack/react-query';
import { api, clearOfflineApiCache } from '../api/client';
import { Button, Card, CardHeader, SampleDataChip, Table, TextField, TopBar } from '../components';
import { usePageTitle } from '../lib/hooks';

// STUB (stage 3a/3m wire it to GET/PATCH /api/settings and GET/POST/DELETE /api/tokens):
// static values. Only "Log out" is real.

const TOKENS = [
  { id: 't1', name: 'Pixel 8 (phone app)', created: '2 Nov 2026', lastUsed: 'Today 07:42' },
  { id: 't2', name: 'Test device', created: '14 Oct 2026', lastUsed: null },
];

export function SettingsPage() {
  usePageTitle('Settings');
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const [busy, setBusy] = useState(false);

  async function logout() {
    setBusy(true);
    try {
      await api.post('/api/logout', undefined, { redirectOn401: false });
    } catch {
      // Already logged out or server unreachable: leave the page either way.
    }
    await clearOfflineApiCache();
    queryClient.clear();
    void navigate('/login', { replace: true });
  }

  return (
    <>
      <TopBar variant="back" backTo="/" title="Settings" subtitle="Account, device tokens, timezone and goals" aside={<SampleDataChip sample />} />
      <div className="page">
        <Card padding="lg">
          <CardHeader title="Device tokens" aside="Phones and tools that upload files" />
          <Table
            caption="Device tokens"
            wide
            rows={TOKENS}
            rowKey={(t) => t.id}
            columns={[
              { key: 'name', header: 'Name', cell: (t) => t.name },
              { key: 'created', header: 'Created', cell: (t) => t.created },
              { key: 'used', header: 'Last used', cell: (t) => t.lastUsed },
              {
                key: 'act',
                header: <span className="sr-only">Actions</span>,
                align: 'end',
                cell: (t) => <Button aria-label={`Revoke token ${t.name}`}>Revoke</Button>,
              },
            ]}
          />
          <Button variant="dashed" size="lg">
            Create token
          </Button>
        </Card>
        <Card padding="lg">
          <CardHeader title="Timezone" />
          <TextField label="Timezone (IANA name)" defaultValue="Europe/Copenhagen" hint="Days are local days in this timezone." autoComplete="off" />
        </Card>
        <Card padding="lg">
          <CardHeader title="Goals" />
          <TextField label="Daily step goal" type="number" inputMode="numeric" defaultValue="8000" />
          <TextField label="Weekly intensity minutes goal" type="number" inputMode="numeric" defaultValue="150" />
          <Button size="lg" variant="primary">
            Save settings
          </Button>
        </Card>
        <Card padding="lg">
          <CardHeader title="Session" />
          <Button size="lg" onClick={() => void logout()} disabled={busy}>
            Log out
          </Button>
        </Card>
      </div>
    </>
  );
}
