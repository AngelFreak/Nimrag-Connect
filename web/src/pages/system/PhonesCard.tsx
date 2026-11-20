import { useState } from 'react';
import type { TokenInfo } from '../../api/generated/TokenInfo';
import { Button, Card, CardHeader, ErrorState, Skeleton } from '../../components';
import { fmtDateTime } from '../today/format';
import { errorMessage } from './errors';
import { useRevokeToken, useTokens } from './queries';

function SignOutButton({ phone }: { phone: TokenInfo }) {
  const revoke = useRevokeToken();
  const [confirm, setConfirm] = useState(false);
  if (!confirm) {
    return (
      <Button aria-label={`Sign out this phone, ${phone.name}`} onClick={() => setConfirm(true)}>
        Sign out this phone
      </Button>
    );
  }
  return (
    <>
      <Button aria-label={`${revoke.isError ? 'Try again' : 'Yes, sign it out'}, ${phone.name}`} onClick={() => revoke.mutate(phone.id, { onSuccess: () => setConfirm(false) })} disabled={revoke.isPending}>
        {revoke.isError ? 'Try again' : 'Yes, sign it out'}
      </Button>
      <Button aria-label={`Cancel, ${phone.name}`} onClick={() => setConfirm(false)}>
        Cancel
      </Button>
    </>
  );
}

/**
 * Settings, "Signed-in phones": the phones that signed in to the app with this account. A phone
 * gets its upload credential in the background when it signs in; nobody creates or copies a token.
 * Signing a phone out revokes that credential on the server, which also ends its sessions and
 * its fingerprint unlock, so the phone has to sign in again with username, password and code.
 *
 * A list, not a table: one phone is a block that wraps (a long name breaks, the buttons drop below
 * the text), so the card never grows wider than a 320 px screen.
 */
export function PhonesCard({ timezone }: { timezone: string }) {
  const query = useTokens();
  const phones = (query.data?.tokens ?? []).filter((t) => t.active);

  return (
    <Card padding="lg" aria-labelledby="phones-title" className="phones-card">
      <CardHeader title="Signed-in phones" id="phones-title" aside="Phones that upload your watch files" />
      {query.isPending ? <Skeleton height={20} lines={3} /> : null}
      {query.isError && !query.data ? (
        <ErrorState title="Could not load the phones" action={<Button onClick={() => void query.refetch()}>Try again</Button>}>
          {errorMessage(query.error)}
        </ErrorState>
      ) : null}
      {query.data ? (
        phones.length === 0 ? (
          <p className="stat__sub">No phone is signed in. Open the Nimrag Connect app on your phone and sign in with your username, password and code.</p>
        ) : (
          <ul className="phones" aria-label="Signed-in phones">
            {phones.map((t) => (
              <li key={t.id} className="phones__item">
                <p className="phones__name">{t.name}</p>
                <dl className="phones__meta">
                  <div>
                    <dt>Signed in</dt>
                    <dd>{fmtDateTime(t.created_at, timezone)}</dd>
                  </div>
                  <div>
                    <dt>Last used</dt>
                    <dd>{fmtDateTime(t.last_used_at, timezone) ?? 'Never'}</dd>
                  </div>
                </dl>
                <div className="phones__actions">
                  <SignOutButton phone={t} />
                </div>
              </li>
            ))}
          </ul>
        )
      ) : null}
    </Card>
  );
}
