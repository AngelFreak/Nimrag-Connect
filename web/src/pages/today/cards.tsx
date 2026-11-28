import type { HomeCard, HomeCards } from '../../api/generated';

/** Name shown on the card and in the Edit home list. Our words, not the watch maker's. */
export const CARD_LABEL: Record<HomeCard, string> = {
  energy: 'Energy',
  heart_rate: 'Heart rate',
  stress: 'Stress',
  sleep: 'Sleep',
  hrv: 'Overnight HRV',
  training_status: 'Training status',
  intensity_minutes: 'Intensity minutes',
  training_load: 'Training load',
  steps: 'Steps',
  recovery_time: 'Recovery time',
  vo2_max: 'VO2 max',
  respiration: 'Breathing',
  spo2: 'Blood oxygen',
  weight: 'Body weight',
  training_readiness: 'Readiness',
  health_status: 'Health status',
};

/** Detail page each card opens. */
export const CARD_LINK: Record<HomeCard, string> = {
  energy: '/metrics/energy',
  heart_rate: '/metrics/heart_rate',
  stress: '/metrics/stress',
  sleep: '/sleep',
  hrv: '/hrv',
  training_status: '/training/status',
  intensity_minutes: '/metrics/intensity_minutes',
  training_load: '/metrics/training_load',
  steps: '/metrics/steps',
  // Recovery time is part of Training readiness; it has no page of its own.
  recovery_time: '/training',
  vo2_max: '/metrics/vo2_max',
  respiration: '/metrics/respiration',
  spo2: '/metrics/spo2',
  weight: '/body',
  training_readiness: '/training/readiness',
  health_status: '/health/status',
};

/** The server's default layout (settings.rs), used by "Reset to default". */
export const DEFAULT_HOME_CARDS: HomeCards = {
  order: [
    'energy',
    'training_readiness',
    'sleep',
    'heart_rate',
    'stress',
    'hrv',
    'training_status',
    'intensity_minutes',
    'training_load',
    'steps',
    'recovery_time',
    'vo2_max',
    'respiration',
    'spo2',
    'weight',
    'health_status',
  ],
  hidden: ['respiration', 'spo2', 'weight'],
};

/** The phone's "In focus" row: these cards are swipeable slides, not tiles. */
export const IN_FOCUS_CARDS: readonly HomeCard[] = ['energy', 'training_readiness', 'sleep', 'hrv'];

/** The cards to show, in the owner's order, without the hidden ones. */
export function visibleCards(cards: HomeCards): HomeCard[] {
  const hidden = new Set(cards.hidden);
  return cards.order.filter((id) => !hidden.has(id));
}

/**
 * Desktop: the "In Focus" grid shows these cards of the layout (when not hidden in Edit Home), next to
 * the two weekly activity cards that have no entry in the layout. "At a Glance" shows the rest.
 */
export const DESKTOP_FOCUS_CARDS: readonly HomeCard[] = ['energy', 'steps', 'sleep', 'training_status'];
