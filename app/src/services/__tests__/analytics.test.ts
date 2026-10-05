import { describe, expect, it } from 'vitest';

import {
  ALLOWED_EVENTS,
  initGA,
  initSentry,
  isAnalyticsEnabled,
  startUiInteractionTracking,
  syncAnalyticsConsent,
  trackAnalyticsEvent,
  trackEvent,
  trackPageView,
} from '../analytics';

// LOCAL-ONLY FORK: analytics is a no-op module. Nothing may be loaded or sent.
describe('analytics (local-only no-op)', () => {
  it('reports analytics as disabled', () => {
    expect(isAnalyticsEnabled()).toBe(false);
  });

  it('never injects a third-party script or global', () => {
    initSentry();
    initGA();
    syncAnalyticsConsent(true);
    trackPageView('/home');
    trackEvent('ui_click', { id: 'x' });
    trackAnalyticsEvent('ui_click', { id: 'x' });
    expect(document.querySelectorAll('script[src]').length).toBe(0);
    expect((window as unknown as Record<string, unknown>).gtag).toBeUndefined();
    expect((window as unknown as Record<string, unknown>).dataLayer).toBeUndefined();
  });

  it('returns a callable cleanup from startUiInteractionTracking', () => {
    const stop = startUiInteractionTracking();
    expect(typeof stop).toBe('function');
    expect(() => stop()).not.toThrow();
  });


  it('keeps the event allowlist so typed call sites still compile', () => {
    expect(ALLOWED_EVENTS.has('ui_click')).toBe(true);
  });
});
