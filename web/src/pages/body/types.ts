// The generated index exports only the top-level request and response types; the nested ones the
// pages need are read off them, so the Rust structs stay the single definition.
import type { BodyResponse, CalendarResponse, MovementResponse } from '../../api/generated';

export type CalendarDay = CalendarResponse['days'][number];
export type ActivityListItem = CalendarDay['activities'][number];
export type DateValue = MovementResponse['week_steps'][number];
export type TimedValue = BodyResponse['weights'][number];
