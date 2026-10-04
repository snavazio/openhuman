/**
 * Analytics service — LOCAL-ONLY FORK.
 *
 * Upstream initialised Sentry, Google Analytics 4 and OpenPanel here. This
 * build sends nothing anywhere: every export keeps its name and signature so
 * callers are unchanged, but the functions are no-ops. The allowlist and the
 * event-name type are kept so `trackAnalyticsEvent` call sites still
 * type-check.
 */

export type AnalyticsParams = Record<string, string | number | boolean>;

const ALLOWED_EVENT_NAMES = [
  'app_open',
  'onboarding_start',
  'onboarding_step_complete',
  'onboarding_complete',
  'account_connect_start',
  'account_connect_success',
  'chat_message_sent',
  'chat_message_shared',
  'github_star_cta_clicked',
  'github_star_cta_dismissed',
  'automation_run_started',
  'automation_run_resumed',
  'automation_run_cancelled',
  'memory_engine_switched',
  'memory_repair_succeeded',
  'memory_tree_retry_succeeded',
  'skill_install',
  'skill_uninstall',
  'tab_bar_change',
  'tauri_browser_click',
  'ui_click',
  'ui_control_change',
  'ui_form_submit',
] as const;

export type AnalyticsEventName = (typeof ALLOWED_EVENT_NAMES)[number];
export const ALLOWED_EVENTS: ReadonlySet<string> = new Set(ALLOWED_EVENT_NAMES);

/** Analytics never runs in this build. */
export function isAnalyticsEnabled(): boolean {
  return false;
}

/** No-op: Sentry is removed from this build. */
export function initSentry(): void {}

/** No-op: there is no analytics provider to sync consent with. */
export function syncAnalyticsConsent(_enabled: boolean): void {}

/** No-op: Google Analytics is removed from this build. */
export function initGA(): void {}

/** No-op: page views are not recorded. */
export function trackPageView(_path: string): void {}

/** No-op: events are not recorded. */
export function trackEvent(_eventName: string, _params?: AnalyticsParams): void {}

/** No-op: events are not recorded. */
export function trackAnalyticsEvent(
  _eventName: AnalyticsEventName,
  _params?: AnalyticsParams
): void {}

/** No-op: returns a no-op cleanup so callers can still unsubscribe. */
export function startUiInteractionTracking(): () => void {
  return () => {};
}

/** Sentry is removed, so there is never an event id. */
export async function triggerSentryTestEvent(): Promise<string | undefined> {
  return undefined;
}
