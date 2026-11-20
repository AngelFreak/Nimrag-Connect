import { useLogout, useMe } from '../../account/queries';
import { Button, Card, CardHeader, Chip } from '../../components';

/** Settings, "Signed in": who this browser or app is logged in as, and Log out. */
export function AccountCard() {
  const me = useMe();
  const logout = useLogout();
  return (
    <Card padding="lg" aria-labelledby="session-title">
      <CardHeader title="Signed in" id="session-title" level={3} />
      <div className="account">
        <div className="account__who">
          <span className="account__name">
            {me.data?.username ?? 'Your account'}
            {me.data ? <Chip>{me.data.role === 'admin' ? 'Admin' : 'User'}</Chip> : null}
          </span>
        </div>
        <Button onClick={() => logout.mutate()} disabled={logout.isPending}>
          Log out
        </Button>
      </div>
    </Card>
  );
}
