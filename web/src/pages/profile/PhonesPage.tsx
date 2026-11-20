import { useState } from 'react';
import type { TokenInfo } from '../../api/generated/TokenInfo';
import { Button, Card, ConfirmDialog, ErrorState, PhoneIcon, SettingsGroup, SettingsRow, Skeleton } from '../../components';
import { fmtDateTime } from '../today/format';
import { useSettings } from '../today/queries';
import { errorMessage } from '../system/errors';
import { useRevokeToken, useTokens } from '../system/queries';
import { activePhones, SubPage } from './common';

/** "Signed in 2 Nov, 10:00 · last upload 19 Nov, 07:42": the date tells two phones with one name apart. */
export function phoneLine(phone: TokenInfo, timezone: string): string {
  const signedIn = fmtDateTime(phone.created_at, timezone);
  const lastUpload = fmtDateTime(phone.last_used_at, timezone);
  return `Signed in ${signedIn ?? 'at an unknown time'} · ${lastUpload ? `last upload ${lastUpload}` : 'no upload yet'}`;
}

function SignOut({ phone, onDone, onCancel }: { phone: TokenInfo; onDone: () => void; onCancel: () => void }) {
  const revoke = useRevokeToken();
  return (
    <ConfirmDialog
      title={`Sign out ${phone.name}?`}
      consequence={
        phone.current
          ? 'This is the phone you are using now. It stops uploading and has to sign in again with your username, password and a code.'
          : 'It stops uploading and its fingerprint unlock stops working. It has to sign in again with your username, password and a code.'
      }
      confirmLabel="Sign out"
      pending={revoke.isPending}
      error={revoke.isError ? errorMessage(revoke.error) : undefined}
      onCancel={onCancel}
      onConfirm={() => revoke.mutate(phone.id, { onSuccess: onDone })}
    />
  );
}

/**
 * Profile, Signed-in phones: the phones that signed in to the app with this account. A phone gets its
 * upload credential in the background when it signs in; nobody creates or copies a token. Signing a
 * phone out revokes that credential on the server, which also ends its sessions and its fingerprint
 * unlock.
 */
export function PhonesPage() {
  const query = useTokens();
  const settings = useSettings();
  const timezone = settings.data?.timezone ?? Intl.DateTimeFormat().resolvedOptions().timeZone;
  const phones = activePhones(query.data?.tokens);
  const [asking, setAsking] = useState<TokenInfo | null>(null);

  return (
    <SubPage title="Signed-in phones" backTo="/profile">
      <p className="subpage__lede">Phones that signed in to the Nimrag Connect app with your account. They upload your watch files.</p>
      {query.isPending ? (
        <Card padding="lg" aria-label="Loading the phones">
          <Skeleton height={20} lines={3} />
        </Card>
      ) : null}
      {query.isError && !query.data ? (
        <Card padding="lg">
          <ErrorState title="Could not load the phones" action={<Button onClick={() => void query.refetch()}>Try again</Button>}>
            {errorMessage(query.error)}
          </ErrorState>
        </Card>
      ) : null}
      {query.data && phones.length === 0 ? <p className="subpage__lede">No phone is signed in. Open the Nimrag Connect app on your phone and sign in with your username, password and code.</p> : null}
      {phones.length > 0 ? (
        <SettingsGroup title="Phones">
          {phones.map((p) => (
            <SettingsRow
              key={p.id}
              icon={<PhoneIcon />}
              tone="sleep"
              title={p.current ? `${p.name} (this phone)` : p.name}
              value={phoneLine(p, timezone)}
              action={
                <Button aria-label={`Sign out ${p.name}, signed in ${fmtDateTime(p.created_at, timezone) ?? 'at an unknown time'}`} onClick={() => setAsking(p)}>
                  Sign out
                </Button>
              }
            />
          ))}
        </SettingsGroup>
      ) : null}
      {asking ? <SignOut phone={asking} onCancel={() => setAsking(null)} onDone={() => setAsking(null)} /> : null}
    </SubPage>
  );
}
