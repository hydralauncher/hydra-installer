import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { motion, AnimatePresence } from "framer-motion";
import { useTranslation } from "react-i18next";
import { CheckIcon } from "@heroicons/react/24/solid";
import LanguageSelector from "./components/LanguageSelector";
import "./App.css";

interface DownloadProgress {
  downloaded: number;
  total: number | null;
  percentage: number;
  speed: number; // bytes per second
  eta: number | null; // seconds remaining
}

interface DownloadComplete {
  path: string;
  total?: number;
}

function App() {
  const { t } = useTranslation();
  const [downloading, setDownloading] = useState(false);
  const [installing, setInstalling] = useState(false);
  const [installationComplete, setInstallationComplete] = useState(false);
  const [progress, setProgress] = useState(0);
  const [downloaded, setDownloaded] = useState(0);
  const [totalSize, setTotalSize] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [logoFocused, setLogoFocused] = useState(false);
  const [logoMinimized, setLogoMinimized] = useState(false);
  const [animationKey, setAnimationKey] = useState(0);
  const [contentVisible, setContentVisible] = useState(false);
  const [hasPreviousInstallation, setHasPreviousInstallation] = useState(false);
  const [deletePreviousInstallation, setDeletePreviousInstallation] = useState(false);

  function scheduleAnimation(callbacks: Array<{ time: number; callback: () => void }>) {
    const startTime = performance.now();
    let animationFrameId: number;
    const executed = new Set<number>();

    function animate(currentTime: number) {
      const elapsed = currentTime - startTime;

      callbacks.forEach(({ time, callback }) => {
        if (elapsed >= time && !executed.has(time)) {
          callback();
          executed.add(time);
        }
      });

      if (executed.size < callbacks.length) {
        animationFrameId = requestAnimationFrame(animate);
      }
    }

    animationFrameId = requestAnimationFrame(animate);
    return () => cancelAnimationFrame(animationFrameId);
  }

  // Helper function to format bytes
  function formatBytes(bytes: number): string {
    if (bytes === 0) return "0 B";
    const k = 1024;
    const sizes = ["B", "KB", "MB", "GB", "TB"];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return Math.round(bytes / Math.pow(k, i)) + " " + sizes[i];
  }


  async function startDownload() {
    try {
      if (deletePreviousInstallation) {
        await invoke("delete_previous_installation");
      }
      
      setDownloading(true);
      setInstalling(false);
      setProgress(0);
      setDownloaded(0);
      setTotalSize(null);
      setError(null);
      const url = "https://github.com/hydralauncher/hydra/releases/download/v3.7.6/hydralauncher-3.7.6-setup.exe";
      await invoke("start_download", { url });
    } catch (err) {
      setError(err as string);
      setDownloading(false);
      setInstalling(false);
    }
  }

  useEffect(() => {
    async function showWindow() {
      try {
        await invoke("show_main_window");
      } catch (err) {
        console.error("Error showing main window:", err);
      }
    }
    showWindow();

    const cancelAnimation = scheduleAnimation([
      { time: 100, callback: () => setLogoFocused(true) },
      { time: 1600, callback: () => setLogoMinimized(true) },
      { time: 2500, callback: () => setContentVisible(true) },
    ]);

    return () => {
      cancelAnimation();
    };
  }, []);

  useEffect(() => {
    async function checkInstallation() {
      try {
        const exists = await invoke<boolean>("check_previous_installation");
        setHasPreviousInstallation(exists);
      } catch (err) {
        console.error("Error checking previous installation:", err);
      }
    }
    checkInstallation();
  }, []);

  useEffect(() => {
    const unlistenProgress = listen<DownloadProgress>("download-progress", (event) => {
      const progressData = event.payload;
      if (progressData.percentage >= 0) {
        setProgress(progressData.percentage);
      }
      setDownloaded(progressData.downloaded);
      if (progressData.total !== null && progressData.total !== undefined) {
        setTotalSize(progressData.total);
      }
    });

    const unlistenComplete = listen<DownloadComplete>("download-complete", (event) => {
      setDownloading(false);
      setProgress(100);
      setInstalling(true);
      if (event.payload.total) {
        setTotalSize(event.payload.total);
      }
    });

    const unlistenInstallComplete = listen("install-complete", async () => {
      setInstallationComplete(true);
      
      try {
        await invoke("launch_hydra");
        await new Promise(resolve => setTimeout(resolve, 1000));
        await invoke("exit_app");
      } catch (err) {
        console.error("Failed to launch Hydra or exit app:", err);
        setError(`Failed to launch Hydra: ${err}`);
      }
    });

    const unlistenError = listen<{ error: string }>("download-error", (event) => {
      setError(event.payload.error);
      setDownloading(false);
      setInstalling(false);
    });

    const unlistenInstallError = listen<{ error: string }>("install-error", (event) => {
      setError(event.payload.error);
      setInstalling(false);
    });

    return () => {
      unlistenProgress.then((unlisten) => unlisten());
      unlistenComplete.then((unlisten) => unlisten());
      unlistenInstallComplete.then((unlisten) => unlisten());
      unlistenError.then((unlisten) => unlisten());
      unlistenInstallError.then((unlisten) => unlisten());
    };
  }, []);

  return (
    <>
      <video
        className="background-video"
        autoPlay
        loop
        muted
        playsInline
      >
        <source src="/hydra-clouds-2.mp4" type="video/mp4" />
      </video>
      <AnimatePresence mode="wait">
        <motion.div
          key={`logo-${animationKey}`}
          className="logo-container"
          initial={{ 
            x: '-50%',
            y: '-50%',
            opacity: 0,
            scale: 1
          }}
          animate={logoMinimized ? {
            x: '-50%',
            y: -350,
            opacity: 1,
            scale: 0.35,
            transition: {
              duration: 1.2,
              ease: [0.4, 0, 0.2, 1]
            }
          } : logoFocused ? {
            x: '-50%',
            y: '-50%',
            opacity: 1,
            scale: 1,
            transition: {
              duration: 1.5,
              ease: [0.4, 0, 0.2, 1]
            }
          } : {
            x: '-50%',
            y: '-50%',
            opacity: 0,
            scale: 1,
            transition: {
              duration: 0.3
            }
          }}
          style={{
            position: 'fixed',
            top: '50%',
            left: '50%',
            zIndex: 10,
            pointerEvents: 'none',
            transformOrigin: 'center center'
          }}
        >
          <motion.img 
            src="/hydra.svg" 
            alt="Hydra Logo" 
            className="hydra-logo"
            style={{
              filter: logoFocused ? 'blur(0px)' : 'blur(20px)',
              width: '224px',
              height: '216px',
              transition: 'filter 1.5s cubic-bezier(0.4, 0, 0.2, 1)'
            }}
          />
        </motion.div>
      </AnimatePresence>
      <main className="container">
        <div className="download-card">
          <div className="download-card-header">
            <motion.div
              initial={{ opacity: 0 }}
              animate={{ 
                opacity: contentVisible ? 1 : 0,
                transition: {
                  duration: 0.8,
                  ease: [0.4, 0, 0.2, 1]
                }
              }}
              style={{ 
                display: 'flex',
                flexDirection: 'column',
                alignItems: 'center',
                gap: '16px',
                width: '100%'
              }}
            >
              <div className="download-indicator-container">
                <div className={`step-orb ${!downloading && !installing && !installationComplete ? 'active' : ''}`}></div>
                <div className={`step-orb ${downloading ? 'active' : ''}`}></div>
                <div className={`step-orb ${installing || installationComplete ? 'active' : ''}`}></div>
              </div>
              <h1 className="download-title">
                {installing ? t("title.installing") : downloading ? t("title.downloading") : t("title.default")}
              </h1>
              <p className="download-description">
                {t("description")}
              </p>
            </motion.div>
          </div>

          <motion.div
            initial={{ opacity: 0 }}
            animate={{ 
              opacity: contentVisible ? 1 : 0,
              transition: {
                duration: 0.8,
                ease: [0.4, 0, 0.2, 1]
              }
            }}
            style={{
              display: 'flex',
              flexDirection: 'column',
              alignItems: 'center',
              justifyContent: 'center',
              width: '100%',
              gap: '16px'
            }}
          >
            {downloading || installing ? (
              <div className="download-progress-section">
                <div className="download-progress-bar">
                  {installing ? (
                    <div className="download-progress-fill-indeterminate"></div>
                  ) : (
                    <div
                      className="download-progress-fill"
                      style={{ width: `${progress}%` }}
                    ></div>
                  )}
                </div>
                <div className="download-progress-info">
                  {installing ? (
                    <>
                      <span className="download-progress-percentage">—</span>
                      <span className="download-progress-size">{t("installing")}</span>
                    </>
                  ) : (
                    <>
                      <span className="download-progress-percentage">{Math.round(progress)}%</span>
                      <span className="download-progress-size">
                        {formatBytes(downloaded)} / {totalSize ? formatBytes(totalSize) : '...'}
                      </span>
                    </>
                  )}
                </div>
              </div>
            ) : (
              <>
                {hasPreviousInstallation && (
                  <label className="delete-previous-checkbox">
                    <div className="custom-checkbox">
                      <input
                        type="checkbox"
                        checked={deletePreviousInstallation}
                        onChange={(e) => setDeletePreviousInstallation(e.target.checked)}
                      />
                      {deletePreviousInstallation && (
                        <div className="checkbox-check">
                          <CheckIcon />
                        </div>
                      )}
                    </div>
                    <span>{t("deletePreviousInstallation")}</span>
                  </label>
                )}
                <button
                  onClick={startDownload}
                  className="download-start-button"
                >
                  {t("startDownload")}
                </button>
              </>
            )}

            {error && (
              <p className="download-error-message">{t("error")}: {error}</p>
            )}

          </motion.div>
        </div>

    </main>
    <LanguageSelector visible={contentVisible} />
    </>
  );
}

export default App;
