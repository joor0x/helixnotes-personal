import assert from 'node:assert/strict';
import test from 'node:test';
import { advanceDate, canonicalRepeat, nextOccurrence, nextOccurrenceText, parseRepeat, repeatLabel, repeatOf } from './task-recurrence.ts';

const at = (stamp) => {
  const [date, time] = stamp.split('T');
  const [y, m, d] = date.split('-').map(Number);
  const [hh, mm] = time.split(':').map(Number);
  return new Date(y, m - 1, d, hh, mm);
};

test('parses and canonicalizes repeat rules like the Rust parser', () => {
  assert.deepEqual(parseRepeat('Weekly'), { unit: 'w', count: 1 });
  assert.deepEqual(parseRepeat('2w'), { unit: 'w', count: 2 });
  assert.equal(parseRepeat('0d'), null);
  assert.equal(parseRepeat('1000d'), null);
  assert.equal(parseRepeat('often'), null);
  assert.equal(canonicalRepeat(parseRepeat('1m')), 'monthly');
  assert.equal(canonicalRepeat(parseRepeat('10d')), '10d');
  assert.deepEqual(repeatOf('Gym repeat:weekdays #health'), { unit: 'weekdays', count: 1 });
  assert.equal(repeatOf('norepeat:daily'), null);
});

test('dates follow the calendar without drifting', () => {
  assert.equal(advanceDate('2026-01-31', parseRepeat('monthly'), 1), '2026-02-28');
  assert.equal(advanceDate('2026-01-31', parseRepeat('monthly'), 2), '2026-03-31');
  assert.equal(advanceDate('2028-02-29', parseRepeat('yearly'), 1), '2029-02-28');
  assert.equal(advanceDate('2026-11-27', parseRepeat('weekdays'), 1), '2026-11-30'); // Fri -> Mon
  assert.equal(advanceDate('2026-03-28', parseRepeat('daily'), 1), '2026-03-29'); // across DST
  assert.equal(advanceDate('2026-12-28', parseRepeat('2w'), 1), '2027-01-11');
});

test('the next occurrence skips periods in the past, so its reminder never fires at once', () => {
  const rule = parseRepeat('daily');
  assert.deepEqual(nextOccurrence('2026-11-30', null, null, rule, at('2026-12-03T08:00')), { due: '2026-12-03', remind: null });
  assert.deepEqual(
    nextOccurrence('2026-11-30', null, '2026-11-30T07:00', rule, at('2026-12-03T08:00')),
    { due: '2026-12-04', remind: '2026-12-04T07:00' },
  );
  assert.deepEqual(
    nextOccurrence('2026-11-30', '14:30', '2026-11-30T13:45', parseRepeat('weekly'), at('2026-11-30T15:00')),
    { due: '2026-12-07T14:30', remind: '2026-12-07T13:45' },
  );
  assert.deepEqual(nextOccurrence(null, null, null, parseRepeat('2w'), at('2026-01-31T10:00')), { due: '2026-02-14', remind: null });
});

test('the next occurrence text keeps every token in place', () => {
  const now = at('2026-11-30T15:00');
  assert.equal(
    nextOccurrenceText('Pay rent !high due:2026-11-30 remind:2026-11-30T09:00 repeat:monthly #home', now),
    'Pay rent !high due:2026-12-30 remind:2026-12-30T09:00 repeat:monthly #home',
  );
  assert.equal(nextOccurrenceText('Water plants repeat:3d', now), 'Water plants repeat:3d due:2026-12-03');
  assert.equal(nextOccurrenceText('One-off due:2026-11-30', now), null);
});

test('labels name the recurrence', () => {
  const labels = {
    daily: 'Daily', weekdays: 'Weekdays', weekly: 'Weekly', monthly: 'Monthly', yearly: 'Yearly',
    every: (count, unit) => `Every ${count}${unit}`,
  };
  assert.equal(repeatLabel('weekly', labels), 'Weekly');
  assert.equal(repeatLabel('3d', labels), 'Every 3d');
  assert.equal(repeatLabel('bogus', labels), 'bogus');
});
