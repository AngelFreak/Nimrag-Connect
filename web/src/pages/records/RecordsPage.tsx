import { Link } from 'react-router-dom';
import type { RecordsResponse } from '../../api/generated';
import type { PersonalRecord } from '../../api/generated/PersonalRecord';
import type { RecordGroup } from '../../api/generated/RecordGroup';
import { Card, CardHeader, PageHeader, SampleDataChip, TopBar } from '../../components';
import { browserToday } from '../../lib/dates';
import { useIsDesktop, usePageTitle } from '../../lib/hooks';
import { activityTitle } from '../activities/format';
import { PageError, PageSkeleton } from '../body/common';
import { useRecords } from './api';
import { fmtRecordDate, fmtRecordValue, groupHasRecords, groupText, recordLabel } from './format';
import './records.css';

/**
 * Personal records: the best of the owner's own activities and step counts, a card per sport group.
 * Each record that came from an activity links to it; a record that beat an older one says what
 * the older one was.
 */
export function RecordsPage() {
  usePageTitle('Records');
  const desktop = useIsDesktop();
  const query = useRecords();
  const data = query.data;

  return (
    <>
      {desktop ? (
        <PageHeader title="Personal records" actions={<SampleDataChip sample={data?.sample} />} />
      ) : (
        <TopBar variant="back" backTo="/activities" backLabel="Back to activities" title="Records" subtitle="Your personal bests" aside={<SampleDataChip sample={data?.sample} />} />
      )}
      {query.isError && !data ? (
        <PageError title="Records could not be loaded" error={query.error} onRetry={() => void query.refetch()} />
      ) : !data ? (
        <PageSkeleton cards={3} />
      ) : (
        <RecordsBody data={data} today={browserToday()} />
      )}
    </>
  );
}

export function RecordsBody({ data, today }: { data: RecordsResponse; today: string }) {
  return (
    <div className="page" style={{ paddingTop: 0 }}>
      <div className="rec-grid">
        {data.groups.map((g) => (
          <GroupCard key={g.key} group={g} today={today} />
        ))}
      </div>
    </div>
  );
}

function GroupCard({ group, today }: { group: RecordGroup; today: string }) {
  const text = groupText(group.key);
  const id = `rec-${group.key}`;
  return (
    <Card padding="list" className="rec-card" aria-labelledby={id}>
      <div className="rec-card__head">
        <CardHeader title={text.title} id={id} />
      </div>
      {groupHasRecords(group.records) ? (
        <ul className="rec-list">
          {group.records.map((r) => (
            <li key={r.kind}>
              <RecordRow record={r} today={today} />
            </li>
          ))}
        </ul>
      ) : (
        <p className="rec-empty">
          <strong>{text.empty.title}</strong>
          {text.empty.body}
        </p>
      )}
    </Card>
  );
}

function RecordRow({ record: r, today }: { record: PersonalRecord; today: string }) {
  const value = fmtRecordValue(r);
  const a = r.activity;
  const when = [fmtRecordDate(r.date, r.kind, today), a ? activityTitle(a) : null].filter(Boolean).join(' · ');
  const before = r.previous ? [fmtRecordValue({ ...r, value: r.previous.value }), fmtRecordDate(r.previous.date, r.kind, today)].filter(Boolean).join(' · ') : null;
  const content = (
    <>
      <span className="rec-row__top">
        <span className="rec-row__label">{recordLabel(r.kind)}</span>
        {value !== null ? <span className="rec-row__value">{value}</span> : <span className="rec-row__none">Not yet</span>}
      </span>
      {value !== null && (when || before) ? (
        <span className="rec-row__bottom">
          {when ? <span className="rec-row__sub">{when}</span> : null}
          {before ? <span className="rec-row__sub rec-row__before">Before: {before}</span> : null}
        </span>
      ) : null}
    </>
  );
  return a ? (
    <Link to={`/activities/${a.id}`} className="rec-row rec-row--link">
      {content}
    </Link>
  ) : (
    <div className="rec-row">{content}</div>
  );
}
