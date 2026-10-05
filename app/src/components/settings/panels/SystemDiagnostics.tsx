// ---------------------------------------------------------------------------
// SystemDiagnostics
//
// Diagnostic callouts (app logs, restart tour) that used to live
// on the standalone Developer & Diagnostics page. Now that the dev pages are
// listed directly in the settings sidebar, these utility rows live on the
// About page instead. Self-contained so About can drop them in as one block.
// ---------------------------------------------------------------------------
import { useEffect, useState } from 'react';
import { useNavigate } from 'react-router-dom';

import { useT } from '../../../lib/i18n/I18nContext';
// `safeInvoke` (aliased to `invoke`) turns the CEF synchronous IPC throw into a
// rejected Promise so the `.catch(...)` handlers see a normal failure.
import { safeInvoke as invoke, isTauri } from '../../../utils/tauriCommands/common';
import Button from '../../ui/Button';
import Card from '../../ui/Card';
import Field from '../../ui/Field';
import { resetWalkthrough } from '../../walkthrough/AppWalkthrough';

const LogsFolderRow = () => {
  const { t } = useT();
  const [path, setPath] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!isTauri()) return;
    invoke<string | null>('logs_folder_path')
      .then(p => setPath(p ?? null))
      .catch(err => setError(err instanceof Error ? err.message : String(err)));
  }, []);

  const onClick = async () => {
    setError(null);
    try {
      await invoke('reveal_logs_folder');
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  };

  if (!isTauri()) return null;

  return (
    <div>
      <Field
        label={t('devOptions.appLogs')}
        description={path ?? t('devOptions.appLogsDesc')}
        control={
          <Button variant="secondary" size="sm" onClick={onClick}>
            {t('devOptions.openLogsFolder')}
          </Button>
        }
      />
      {error && (
        <div
          role="status"
          aria-live="polite"
          className="px-4 pb-3 text-xs text-coral-600 dark:text-coral-300">
          {error}
        </div>
      )}
    </div>
  );
};

const RestartTourRow = () => {
  const { t } = useT();
  const navigate = useNavigate();

  const onClick = () => {
    resetWalkthrough();
    navigate('/home');
  };

  return (
    <Field
      label={t('settings.restartTour')}
      description={t('settings.restartTourDesc')}
      control={
        <Button variant="secondary" size="sm" onClick={onClick}>
          {t('settings.restartTour')}
        </Button>
      }
    />
  );
};

/** App logs and the restart-tour action. */
const SystemDiagnostics = () => {
  const { t } = useT();

  return (
    <>
      <Card title={t('devOptions.titleDiagnostics')} data-testid="about-diagnostics">
        <LogsFolderRow />
        <RestartTourRow />
      </Card>
      {/* Staging-only, and loud on purpose: it sends a real event. */}
    </>
  );
};

export default SystemDiagnostics;
