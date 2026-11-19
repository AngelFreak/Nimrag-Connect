import { EmptyState, LinkButton, TopBar } from '../components';
import { usePageTitle } from '../lib/hooks';

export function NotFoundPage() {
  usePageTitle('Page not found');
  return (
    <>
      <TopBar variant="back" backTo="/" title="Page not found" />
      <div className="page">
        <EmptyState title="This page does not exist" action={<LinkButton to="/" variant="primary">Go to Today</LinkButton>}>
          The address may be mistyped, or the page was moved.
        </EmptyState>
      </div>
    </>
  );
}
