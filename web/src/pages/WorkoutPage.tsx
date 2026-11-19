import { Button, SampleDataChip, TextField, TopBar } from '../components';
import { MoreIcon } from '../components/icons';
import { usePageTitle } from '../lib/hooks';

// STUB (stage 4, writing to the watch): static canvas steps; nothing is saved or sent.

const REPEATS = [
  { name: 'Dumbbell bench press', detail: '10 reps · 22 kg' },
  { name: 'Bent-over row', detail: '10 reps · 20 kg' },
  { name: 'Overhead press', detail: '8 reps · 14 kg' },
];

export function WorkoutPage() {
  usePageTitle('New workout');
  return (
    <>
      <TopBar variant="back" backTo="/activities?type=strength" title="Workout" subtitle="Strength · about 40 min" aside={<SampleDataChip sample />} />
      <div className="page" style={{ gap: 10 }}>
        <TextField label="Name" defaultValue="Upper body A" />
        <h2 className="section-label" style={{ marginTop: 6 }}>
          Steps
        </h2>
        <div className="step step--simple">
          <div style={{ display: 'flex', flexDirection: 'column', gap: 3 }}>
            <span className="step__name">Warm-up</span>
            <span className="stat__sub">5 min, easy</span>
          </div>
          <button type="button" className="icon-btn" aria-label="Edit warm-up">
            <MoreIcon size={20} />
          </button>
        </div>
        {REPEATS.map((r) => (
          <section className="step step--repeat" key={r.name} aria-label={`Repeat block: ${r.name}`}>
            <div className="step__row">
              <span className="step__repeat">Repeat 3 times</span>
              <button type="button" className="icon-btn" aria-label={`Edit repeat block: ${r.name}`}>
                <MoreIcon size={20} />
              </button>
            </div>
            <div className="step__row">
              <span className="step__name">{r.name}</span>
              <span>{r.detail}</span>
            </div>
            <div className="step__row step__row--muted">
              <span>Rest</span>
              <span>90 s</span>
            </div>
          </section>
        ))}
        <div className="step step--simple">
          <div style={{ display: 'flex', flexDirection: 'column', gap: 3 }}>
            <span className="step__name">Cool-down</span>
            <span className="stat__sub">5 min, easy</span>
          </div>
          <button type="button" className="icon-btn" aria-label="Edit cool-down">
            <MoreIcon size={20} />
          </button>
        </div>
        <Button variant="dashed" size="lg">
          Add step
        </Button>
      </div>
      <div className="footer-bar">
        <Button size="lg">Save</Button>
        <Button size="lg" variant="primary">
          Send to watch
        </Button>
      </div>
    </>
  );
}
